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
        /// A value of the wrong kind or length, or an entry that doesn't parse, or empty once cleaned.
        case invalidValue
        /// A word with characters that hide or reorder text inside it (`TypingConfigText`):
        /// what it would type isn't what it shows.
        case hiddenCharacters
        /// Past the list's limit.
        case tooMany
        /// The same entry again (or a second key block for the same keyboard).
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

    /// The list a part is in, for "Words to keep, number 6".
    enum List: String, Sendable {
        /// Top-level fields (`note`, an unknown key).
        case file
        case keep, neverCorrectTo, fixes, dictation
        /// `keys[]` itself (a block that doesn't parse).
        case keyBlocks
        case aim, zones
    }

    struct Item: Equatable, Sendable {
        var part: Part
        /// Where it is in the file (`words.keep[0]`, `keys[0].zones[1]`).
        var path: String
        var outcome: Outcome
        var list: List
        /// Its index in `list` (0 is the first); nil for a top-level field or an aim letter.
        var position: Int?
        /// The key block it is in, for aim and zones.
        var block: Int?
        /// What the file claims for it, shown as the file's claim.
        var evidence: TypingConfigFile.Evidence?
        /// The note or word as cleaned (`TypingConfigText`): what the sheet shows and the
        /// keyboard loads. For a fix "typo → word", for dictation "heard → meant".
        var text: String?
    }

    /// A key block's keyboard against the keyboards the review was made for.
    struct ContextCheck: Equatable, Sendable {
        var block: Int
        var madeFor: TypingConfigFile.Context
        /// The keyboard it applies on, else the nearest (the fewest differences).
        var keyboard: TypingConfigFile.Context?
        /// Its index in the review's keyboards when it applies there.
        var keyboardIndex: Int?
        /// The fields that differ from `keyboard`; empty when it applies.
        var differences: [String]
        var hitModelMatches: Bool
        var matches: Bool { keyboardIndex != nil }
    }

    /// One key block's aim and zones on the keyboard it applies on.
    struct AppliedBlock: Equatable, Sendable {
        var block: Int
        var keyboard: Int
        var context: TypingConfigFile.Context
        /// By letter, in keycaps (clamped).
        var aim: [Character: CGVector] = [:]
        var zones: [KeyboardZone] = []
    }

    /// What the keyboard uses from a config: its words (cleaned) and, per keyboard, its key positions.
    struct Applied: Equatable, Sendable {
        var keep: [String] = []
        var neverCorrectTo: [String] = []
        var fixes: [TypingConfigFile.FixEntry] = []
        var dictation: [DictationCorrection] = []
        var blocks: [AppliedBlock] = []

        /// The first keyboard's aim (the live one, for the keyboard itself).
        var aim: [Character: CGVector] { blocks.first { $0.keyboard == 0 }?.aim ?? [:] }
        var zones: [KeyboardZone] { blocks.first { $0.keyboard == 0 }?.zones ?? [] }

        var isEmpty: Bool { self == Applied() }
        var hasWords: Bool { !keep.isEmpty || !neverCorrectTo.isEmpty || !fixes.isEmpty || !dictation.isEmpty }
    }

    /// Set when nothing could be read (`notAConfig`, `tooLarge`).
    var refused: Reason?
    var file: TypingConfigFile?
    /// SHA-256 of the file's bytes: its identity in the history (the file's own `id` is a name
    /// its writer gave it, not checked here).
    var contentHash: String
    var items: [Item] = []
    var contexts: [ContextCheck] = []
    /// Everything that applies on the keyboards reviewed for, with every part switched on.
    var applied = Applied()
    /// The note, cleaned and cut to `TypingConfigLimits.noteMaxCharacters`.
    var note: String?

    /// Reads `data` for one live letters layout.
    init(data: Data, keyboard: KeyboardLayout, checksEvidence: Bool = true) {
        self.init(data: data, keyboards: [TypingConfigKeyboard(layout: keyboard)], checksEvidence: checksEvidence)
    }

    /// Reads `data` for the person's keyboards (`TypingConfigSettings.keyboards`). Each key block
    /// applies on the first keyboard it matches; a later block for the same keyboard is a
    /// duplicate, and one that matches none is for another keyboard. `checksEvidence` is false
    /// for `active.json`, which drops the evidence once a person has applied a config.
    init(data: Data, keyboards: [TypingConfigKeyboard], checksEvidence: Bool = true) {
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
        note = review.note(file.note)
        review.words(file.words)
        review.keys(file.keys ?? [], keyboards: keyboards)
        items = review.items
        contexts = review.contexts
        applied = review.applied
    }

    /// The parts that apply, by default all switched on.
    var switchable: [Part] { items.filter { $0.outcome.applies && $0.part != .note }.map(\.part) }

    /// `active.json` for the parts in `switchedOn` that apply: the file's envelope, no note and
    /// no evidence, words and values as cleaned and clamped, and every key block that applies on
    /// one of the reviewed keyboards (the keyboard uses the one for where it is).
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
        out.keys = a.blocks.compactMap { block in
            var aim: [String: TypingConfigFile.Lossy<TypingConfigFile.Aim>] = [:]
            for (letter, v) in block.aim where on.contains(.aim(block: block.block, letter: String(letter))) {
                aim[String(letter)] = .init(.init(offset: [Double(v.dx), Double(v.dy)]))
            }
            let indices = items.compactMap { item -> Int? in
                if case .zone(block.block, let i) = item.part, item.outcome.applies { return i }
                return nil
            }
            let zones = zip(indices, block.zones).filter { on.contains(.zone(block: block.block, index: $0.0)) }.map { _, z in
                TypingConfigFile.Lossy(TypingConfigFile.Zone(key: z.key, edge: z.edge.rawValue, move: Double(z.move)))
            }
            guard !aim.isEmpty || !zones.isEmpty else { return nil }
            return .init(.init(context: block.context, aim: aim, zones: zones))
        }
        return out
    }
}

nonisolated extension TypingConfigReview {
    /// An aim entry as every reader takes it: nil when it doesn't parse (`invalidValue`), else
    /// its offset clamped to `TypingConfigLimits.aimBound`.
    static func aim(letter: String, _ aim: TypingConfigFile.Aim) -> [Double]? {
        guard letter.count == 1, letter.first?.isLetter == true, letter == letter.lowercased(),
              aim.offset.count == 2, aim.offset.allSatisfy(\.isFinite) else { return nil }
        let bound = Double(TypingConfigLimits.aimBound)
        return aim.offset.map { max(-bound, min(bound, $0)) }
    }

    /// A zone as every reader takes it: nil when it doesn't parse (`invalidValue`), else with
    /// its move clamped to `TypingConfigLimits.zoneMaxMove`.
    static func zone(_ zone: TypingConfigFile.Zone) -> KeyboardZone? {
        guard let edge = KeyboardZone.Edge(rawValue: zone.edge), zone.move.isFinite else { return nil }
        let bound = Double(TypingConfigLimits.zoneMaxMove)
        return KeyboardZone(key: zone.key, edge: edge, move: CGFloat(max(-bound, min(bound, zone.move))))
    }
}

/// Walks the file in its order, recording an item per part.
private nonisolated struct Builder {
    typealias R = TypingConfigReview
    typealias Evidence = TypingConfigFile.Evidence
    let newer: Bool
    let checksEvidence: Bool
    var items: [R.Item] = []
    var contexts: [R.ContextCheck] = []
    var applied = R.Applied()

    init(newer: Bool, checksEvidence: Bool) {
        self.newer = newer
        self.checksEvidence = checksEvidence
    }

    mutating func add(_ part: R.Part, _ path: String, _ outcome: R.Outcome, list: R.List, position: Int? = nil,
                      block: Int? = nil, evidence: Evidence? = nil, text: String? = nil) {
        items.append(R.Item(part: part, path: path, outcome: outcome, list: list, position: position, block: block,
                            evidence: evidence, text: text))
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
                    let (list, position, block) = Self.location(here)
                    add(.unreadable(path: here), here, .skipped(newer ? .newerFormat : .unknownField), list: list, position: position, block: block)
                } else if let next = Self.childLevel(level, key), let child = object[key] {
                    unknownKeys(child, level: next, path: here)
                }
            }
        } else if let array = value as? [Any] {
            for (i, element) in array.enumerated() { unknownKeys(element, level: level + "[]", path: "\(path)[\(i)]") }
        }
    }

    /// The list, position and block a path is in (`words.keep[3].colour`: keep, 3).
    static func location(_ path: String) -> (R.List, Int?, Int?) {
        func index(after prefix: String) -> Int? {
            guard let r = path.range(of: prefix) else { return nil }
            return Int(path[r.upperBound...].prefix { $0.isNumber })
        }
        let block = index(after: "keys[")
        for (name, list) in [("words.keep[", R.List.keep), ("words.neverCorrectTo[", .neverCorrectTo), ("words.fixes[", .fixes), ("words.dictation[", .dictation)] {
            if path.hasPrefix(name) { return (list, index(after: name), nil) }
        }
        if path.contains(".zones[") { return (.zones, index(after: ".zones["), block) }
        if path.contains(".aim.") { return (.aim, nil, block) }
        if path.hasPrefix("keys[") { return (.keyBlocks, block, block) }
        return (.file, nil, nil)
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

    /// The cleaned note, cut to the limit.
    mutating func note(_ note: String?) -> String? {
        guard let note else { return nil }
        let cleaned = TypingConfigText.clean(note).text
        let limit = TypingConfigLimits.noteMaxCharacters
        let cut = String(cleaned.prefix(limit))
        add(.note, "note", cleaned.count > limit ? .shortened(from: cleaned.count, to: limit) : .applied, list: .file, text: cut)
        return cut
    }

    private func enough(_ e: Evidence?, _ check: (Evidence) -> Bool) -> Bool {
        !checksEvidence || e.map(check) == true
    }

    /// A word cleaned, or why it can't be one: empty or the wrong length (invalid), or hidden
    /// characters removed from inside it. `words` is how many words it may be.
    private static func word(_ raw: String, words: ClosedRange<Int> = 1...1) -> (String?, R.Reason?) {
        let (text, removed) = TypingConfigText.clean(raw)
        if removed { return (text.isEmpty ? nil : text, .hiddenCharacters) }
        let count = text.split(separator: " ").count
        guard !text.isEmpty, words.contains(count),
              words.upperBound > 1 ? text.count <= 96 : TypingConfigLimits.wordLetters.contains(text.count) else { return (nil, .invalidValue) }
        return (text, nil)
    }

    mutating func words(_ words: TypingConfigFile.Words?) {
        guard let words else { return }
        var seen = Set<String>()
        for (i, entry) in (words.keep ?? []).enumerated() {
            let path = "words.keep[\(i)]"
            guard let e = entry.value else { add(.unreadable(path: path), path, .skipped(.invalidValue), list: .keep, position: i); continue }
            let (w, bad) = Self.word(e.word)
            guard let w, bad == nil else {
                add(.unreadable(path: path), path, .skipped(bad ?? .invalidValue), list: .keep, position: i, evidence: e.evidence, text: w); continue
            }
            let outcome: R.Outcome = if i >= TypingConfigLimits.maxKeep { .skipped(.tooMany) }
                else if !seen.insert("k" + w).inserted { .skipped(.duplicate) }
                else if !enough(e.evidence, { ($0.times ?? 0) >= TypingConfigLimits.keepMinTimes && ($0.sessions ?? 0) >= TypingConfigLimits.keepMinSessions }) { .skipped(.notEnoughEvidence) }
                else { .applied }
            add(.keep(w), path, outcome, list: .keep, position: i, evidence: e.evidence, text: w)
            if outcome.applies { applied.keep.append(w) }
        }
        for (i, entry) in (words.neverCorrectTo ?? []).enumerated() {
            let path = "words.neverCorrectTo[\(i)]"
            guard let e = entry.value else { add(.unreadable(path: path), path, .skipped(.invalidValue), list: .neverCorrectTo, position: i); continue }
            let (w, bad) = Self.word(e.word)
            guard let w, bad == nil else {
                add(.unreadable(path: path), path, .skipped(bad ?? .invalidValue), list: .neverCorrectTo, position: i, evidence: e.evidence, text: w); continue
            }
            let outcome: R.Outcome = if i >= TypingConfigLimits.maxNeverCorrectTo { .skipped(.tooMany) }
                else if !seen.insert("n" + w).inserted { .skipped(.duplicate) }
                else if !enough(e.evidence, { ($0.takenBack ?? 0) >= TypingConfigLimits.neverCorrectToMinTakenBack }) { .skipped(.notEnoughEvidence) }
                else { .applied }
            add(.neverCorrectTo(w), path, outcome, list: .neverCorrectTo, position: i, evidence: e.evidence, text: w)
            if outcome.applies { applied.neverCorrectTo.append(w) }
        }
        for (i, entry) in (words.fixes ?? []).enumerated() {
            let path = "words.fixes[\(i)]"
            guard let e = entry.value else { add(.unreadable(path: path), path, .skipped(.invalidValue), list: .fixes, position: i); continue }
            let (typo, badTypo) = Self.word(e.typo), (w, badWord) = Self.word(e.word)
            let shown = "\(typo ?? "") → \(w ?? "")"
            guard let typo, let w, badTypo == nil, badWord == nil, typo.lowercased() != w.lowercased() else {
                add(.unreadable(path: path), path, .skipped(badTypo ?? badWord ?? .invalidValue), list: .fixes, position: i, evidence: e.evidence, text: shown); continue
            }
            let outcome: R.Outcome = if i >= TypingConfigLimits.maxFixes { .skipped(.tooMany) }
                else if !seen.insert("f" + typo.lowercased()).inserted { .skipped(.duplicate) }
                else if !enough(e.evidence, { ($0.times ?? 0) >= TypingConfigLimits.fixMinTimes && ($0.sessions ?? 0) >= TypingConfigLimits.fixMinSessions }) { .skipped(.notEnoughEvidence) }
                else { .applied }
            add(.fix(typo: typo, word: w), path, outcome, list: .fixes, position: i, evidence: e.evidence, text: shown)
            if outcome.applies { applied.fixes.append(TypingConfigFile.FixEntry(typo: typo, word: w, mode: e.mode)) }
        }
        for (i, entry) in (words.dictation ?? []).enumerated() {
            let path = "words.dictation[\(i)]"
            guard let e = entry.value else { add(.unreadable(path: path), path, .skipped(.invalidValue), list: .dictation, position: i); continue }
            let range = 1...TypingConfigLimits.dictationMaxWords
            let (heard, badHeard) = Self.word(e.heard, words: range), (meant, badMeant) = Self.word(e.meant, words: range)
            let shown = "\(heard ?? "") → \(meant ?? "")"
            guard let heard, let meant, badHeard == nil, badMeant == nil, heard != meant else {
                add(.unreadable(path: path), path, .skipped(badHeard ?? badMeant ?? .invalidValue), list: .dictation, position: i, evidence: e.evidence, text: shown); continue
            }
            let outcome: R.Outcome = if i >= TypingConfigLimits.maxDictation { .skipped(.tooMany) }
                else if !seen.insert("d" + heard.lowercased()).inserted { .skipped(.duplicate) }
                else if !enough(e.evidence, { ($0.times ?? 0) >= TypingConfigLimits.dictationMinTimes }) { .skipped(.notEnoughEvidence) }
                else { .applied }
            add(.dictation(heard: heard, meant: meant), path, outcome, list: .dictation, position: i, evidence: e.evidence, text: shown)
            if outcome.applies { applied.dictation.append(DictationCorrection(heard: heard, meant: meant)) }
        }
    }

    mutating func keys(_ blocks: [TypingConfigFile.Lossy<TypingConfigFile.KeyBlock>], keyboards: [TypingConfigKeyboard]) {
        var taken = Set<Int>()
        for (b, entry) in blocks.enumerated() {
            let path = "keys[\(b)]"
            guard let block = entry.value else {
                add(.unreadable(path: path), path, .skipped(.invalidValue), list: .keyBlocks, position: b, block: b); continue
            }
            let diffs = keyboards.map { block.context.differences(from: $0.context) }
            let matching = keyboards.indices.filter { diffs[$0].isEmpty }
            let index = matching.first { !taken.contains($0) }
            let nearest = index ?? keyboards.indices.min { diffs[$0].count < diffs[$1].count }
            let check = R.ContextCheck(block: b, madeFor: block.context, keyboard: nearest.map { keyboards[$0].context },
                                       keyboardIndex: b < TypingConfigLimits.maxContexts ? index : nil,
                                       differences: nearest.map { diffs[$0] } ?? [], hitModelMatches: block.context.hitModel == KeyboardZone.hitModel)
            contexts.append(check)
            let blockReason: R.Reason? = b >= TypingConfigLimits.maxContexts ? .tooMany
                : index != nil ? nil : !matching.isEmpty ? .duplicate : .forAnotherKeyboard
            if let index, blockReason == nil { taken.insert(index) }
            let geometry = (blockReason == nil ? index : nil).map { keyboards[$0].geometry }
            var appliedBlock = R.AppliedBlock(block: b, keyboard: index ?? -1, context: block.context)
            for letter in (block.aim ?? [:]).keys.sorted() {
                let aimPath = "\(path).aim.\(letter)"
                guard let aim = block.aim?[letter]?.value, let to = R.aim(letter: letter, aim) else {
                    add(.unreadable(path: aimPath), aimPath, .skipped(.invalidValue), list: .aim, block: b, evidence: block.aim?[letter]?.value?.evidence, text: letter)
                    continue
                }
                let outcome: R.Outcome = if let blockReason { .skipped(blockReason) }
                    else if geometry?.keys.contains(where: { $0.id == letter && $0.row >= 0 }) != true { .skipped(.keyNotOnKeyboard) }
                    else if !enough(aim.evidence, { e in
                        (e.tuned?.taps ?? 0) >= TypingConfigLimits.aimMinTunedTaps && (e.checked?.taps ?? 0) >= TypingConfigLimits.aimMinCheckedTaps
                            && (e.checked?.gained ?? 0) >= (e.checked?.lost ?? Int.max)
                    }) { .skipped(.notEnoughEvidence) }
                    else if to != aim.offset { .clamped(from: aim.offset, to: to) }
                    else { .applied }
                add(.aim(block: b, letter: letter), aimPath, outcome, list: .aim, block: b, evidence: aim.evidence, text: letter)
                if outcome.applies, let c = letter.first { appliedBlock.aim[c] = CGVector(dx: to[0], dy: to[1]) }
            }
            var candidates: [(index: Int, zone: KeyboardZone, outcome: R.Outcome, evidence: Evidence?)] = []
            for (i, entry) in (block.zones ?? []).enumerated() {
                let zonePath = "\(path).zones[\(i)]"
                guard let zone = entry.value, let clamped = R.zone(zone) else {
                    add(.unreadable(path: zonePath), zonePath, .skipped(.invalidValue), list: .zones, position: i, block: b, evidence: entry.value?.evidence)
                    continue
                }
                let to = Double(clamped.move)
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
                candidates.append((i, clamped, outcome, zone.evidence))
            }
            // The geometry: the zones that still apply, together, on the keyboard the block is for.
            let applying = candidates.filter { $0.outcome.applies }
            var geometryReasons: [Int: R.Reason] = [:]
            if let geometry, !applying.isEmpty {
                let map = geometry.zoneMap(applying.map(\.zone))
                for (n, c) in applying.enumerated() { if let reason = map.skipped[n] { geometryReasons[c.index] = reason } }
            }
            for c in candidates {
                let outcome = geometryReasons[c.index].map { R.Outcome.skipped($0) } ?? c.outcome
                let shown = "\(c.zone.key) \(c.zone.edge.rawValue) \(c.zone.move >= 0 ? "+" : "")\(c.zone.move)"
                add(.zone(block: b, index: c.index), "\(path).zones[\(c.index)]", outcome, list: .zones, position: c.index, block: b, evidence: c.evidence, text: shown)
                if outcome.applies { appliedBlock.zones.append(c.zone) }
            }
            if blockReason == nil, index != nil, !appliedBlock.aim.isEmpty || !appliedBlock.zones.isEmpty { applied.blocks.append(appliedBlock) }
        }
    }
}
