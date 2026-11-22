import CoreGraphics
import Foundation
@testable import HEX

/// One case's operations: the engine's setup, then the keyboard's questions and the changes
/// between them.
@MainActor
extension ACParityGenerator {
    struct Case {
        var ops: [[String: Any]] = []
        var learned: [String] = []
    }

    /// The setup a case runs before its questions; `partner` and `decoder` are what it chose.
    mutating func makeCase() -> Case {
        var c = Case()
        let l = language
        var available: Any = "all"
        if rng.chance(0.07) { available = Self.available.filter { !l.acoSpellCheckCandidates.contains($0) } }
        if rng.chance(0.3) { c.learned = (0..<(1 + rng.int(4))).map { _ in rng.chance(0.5) ? word(l).lowercased() : typo(word(l), l) } }
        if rng.chance(0.2) { c.ops.append(["op": "perlang", "on": true]) }
        c.ops.append(["op": "lang", "l": l.rawValue, "avail": available])
        let letters = rng.pick(l.acoLetterLayouts)
        c.ops.append(["op": "keys", "l": l.rawValue, "letters": letters.rawValue])
        if rng.chance(0.1) { c.ops.append(["op": "enabled", "on": false]) }
        if rng.chance(0.25) { c.ops.append(["op": "swear", "on": true]) }
        if rng.chance(0.2) { c.ops.append(["op": "caps", "on": false]) }
        if rng.chance(0.5) { c.ops.append(lexiconOp()) }
        if rng.chance(0.3) { c.ops.append(vocabularyOp()) }
        var partner: KeyboardLanguage?
        if l.isLatin, rng.chance(0.4) {
            let other = rng.chance(0.5) && l != .english ? .english : rng.pick(Self.latin.filter { $0 != l })
            partner = other
            c.ops.append(partnerOp(other, available: available))
        }
        if rng.chance(0.3) { c.ops.append(learningOp(partner: partner)) }
        let hasModel = models[l.rawValue] != nil
        var decoder = false
        if hasModel, rng.chance(0.75) {
            decoder = true
            c.ops.append(["op": "decoder", "model": l.rawValue])
        }
        if hasModel, rng.chance(0.85) { c.ops.append(["op": "context", "model": l.rawValue]) }
        if let partner, models[partner.rawValue] != nil, rng.chance(0.6) {
            decoder = true
            c.ops.append(["op": "pdecoder", "model": partner.rawValue])
        }
        let set = decoder || rng.chance(0.1) ? keyset(l, letters) : nil
        if let set {
            let personal = rng.chance(0.25)
            c.ops.append(["op": "dkeys", "keys": personal ? set.personal : set.stock])
            if personal, rng.chance(0.8) { c.ops.append(["op": "bkeys", "keys": set.stock]) }
        }
        if l == .german {
            if rng.chance(0.5) { c.ops.append(["op": "german", "set": "bundled"]) } else if rng.chance(0.15) { c.ops.append(["op": "german", "set": "none"]) }
        }
        let questions = 3 + rng.int(8)
        for _ in 0..<questions {
            if rng.chance(0.15) { c.ops.append(changeOp(partner: &partner, available: available)) }
            c.ops += question(partner: partner, set: set)
        }
        return c
    }

    mutating func lexiconOp() -> [String: Any] {
        var entries: [[String]] = []
        for _ in 0..<(1 + rng.int(6)) {
            let e = rng.pick(ACParityWords.lexicon)
            entries.append([e.0, e.1])
        }
        return ["op": "lexicon", "entries": entries]
    }

    mutating func vocabularyOp() -> [String: Any] {
        ["op": "vocab", "terms": (0..<rng.int(4)).map { _ in rng.pick(ACParityWords.vocabulary) }]
    }

    mutating func partnerOp(_ other: KeyboardLanguage?, available: Any) -> [String: Any] {
        partnerTags += 1
        return ["op": "partner", "l": other?.rawValue ?? NSNull(), "avail": available, "tag": "x\(partnerTags)", "any": rng.chance(0.1)]
    }

    mutating func learningPairs(_ l: KeyboardLanguage) -> ([[String]], [String]) {
        var typos: [[String]] = []
        var seen = Set<String>()
        for _ in 0..<rng.int(5) {
            let w = word(l).lowercased()
            let t = typo(w, l).lowercased()
            // Two fixes that differ only in case let the order decide the decoder's spelling.
            let fix = rng.chance(0.2) ? w.prefix(1).uppercased() + w.dropFirst() : w
            if t != w, seen.insert(t).inserted { typos.append([t, fix]) }
        }
        if rng.chance(0.3), let first = typos.first {
            let other = typo(first[0], l)
            if seen.insert(other).inserted { typos.append([other, first[1].uppercased() == first[1] ? first[1] : first[1].capitalized]) }
        }
        return (typos, (0..<rng.int(3)).map { _ in rng.chance(0.5) ? word(l).lowercased() : rng.pick(["teh", "dont", "im", "id", "well"]) })
    }

    mutating func learningOp(partner: KeyboardLanguage?) -> [String: Any] {
        let (typos, rejected) = learningPairs(language)
        var op: [String: Any] = ["op": "learning", "typos": typos, "rejected": rejected]
        let (ptypos, prejected) = learningPairs(partner ?? language)
        op["ptypos"] = ptypos
        op["prejected"] = prejected
        return op
    }

    mutating func changeOp(partner: inout KeyboardLanguage?, available: Any) -> [String: Any] {
        switch rng.int(14) {
        case 0: return ["op": "keep", "w": rng.chance(0.5) ? word(language) : typo(word(language), language)]
        case 1: return learningOp(partner: partner)
        case 2: return vocabularyOp()
        case 3: return lexiconOp()
        case 4: return ["op": "swear", "on": rng.chance(0.5)]
        case 5: return ["op": "caps", "on": rng.chance(0.7)]
        case 6: return ["op": "enabled", "on": rng.chance(0.8)]
        case 7: return ["op": "lang", "l": (rng.chance(0.7) ? language : rng.pick(Self.latin)).rawValue, "avail": available]
        case 8:
            let other: KeyboardLanguage? = rng.chance(0.2) ? nil : rng.pick(Self.latin)
            partner = other
            return partnerOp(other, available: available)
        case 9: return ["op": "perlang", "on": rng.chance(0.5)]
        case 10: return ["op": "erase"]
        case 11: return ["op": "taps", "taps": NSNull()]
        case 12: return ["op": "forget"]
        default: return ["op": "decoder", "model": models[language.rawValue] != nil && rng.chance(0.7) ? language.rawValue as Any : NSNull()]
        }
    }

    /// A word being typed (cased, with its taps when Spatial autocorrect reads them) and the
    /// text before it.
    mutating func typing(partner: KeyboardLanguage?, set: KeySet?) -> (before: String, token: String, taps: [CGPoint]?) {
        let before = prefix(language, partner: partner)
        var token = rawToken(language, partner: partner)
        var points: [CGPoint]?
        if let set, rng.chance(0.75) {
            if rng.chance(0.7), let read = taps(for: token, set, spread: rng.pick([0.12, 0.25, 0.4])) {
                token = read.0
                points = read.1
            } else if let read = taps(for: token, set, spread: 0.1) {
                points = read.1
            }
            if rng.chance(0.04), points != nil, !points!.isEmpty { points!.removeLast() }
        }
        return (before, cased(token), points)
    }

    mutating func question(partner: KeyboardLanguage?, set: KeySet?) -> [[String: Any]] {
        let (before, token, taps) = typing(partner: partner, set: set)
        let tapsOp: [String: Any] = ["op": "taps", "taps": taps.map(Self.json) ?? NSNull()]
        let field = rng.chance(0.95) ? "text" : rng.pick(["url", "email"])
        let off = rng.chance(0.04)
        var text: String? = before + token
        if rng.chance(0.1) { text = before + token + " " }
        if rng.chance(0.02) { text = nil }
        if rng.chance(0.03) { text = before + token + rng.pick([",", ".", "!", "1", "'"]) }
        let roll = rng.int(100)
        switch roll {
        case 0..<40:
            let model: Any = models[language.rawValue] != nil && rng.chance(0.85) ? language.rawValue as Any : (rng.chance(0.1) ? "en" as Any : NSNull())
            return [tapsOp, ["op": "suggest", "before": text ?? NSNull(), "field": field, "off": off, "model": model, "cap": rng.chance(0.3)]]
        case 40..<68:
            return [tapsOp, ["op": "space", "before": text ?? NSNull(), "field": field, "off": off]]
        case 68..<77:
            let next = partner != nil && rng.chance(0.5) ? word(partner!) : word(language)
            let w = rng.chance(0.4) ? rng.pick(["id", "ill", "well", "its", "your", "there", "then", "Im", "Dont", "dont", "im", "voce",
                                               "ate", "esta", "amanha", "nao", "cafe", "deja", "ca", "perche", "Its", "Well", "I'd"]) : token
            return [["op": "revision", "w": w, "before": before + w, "next": next + (rng.chance(0.2) ? " " + word(language) : ""), "field": field, "off": off]]
        case 77..<81:
            guard let set else { return [tapsOp, ["op": "space", "before": text ?? NSNull(), "field": field, "off": off]] }
            return mergeOps(before: before, set: set)
        case 81..<83: return [["op": "replacement", "before": text ?? NSNull()]]
        case 83..<85: return [["op": "offer", "w": rng.chance(0.5) ? token : rawToken(language, partner: partner), "after": rng.chance(0.5) ? word(language) as Any : NSNull()]]
        case 85..<87: return [["op": "knows", "w": token]]
        case 87..<89: return [["op": "lean", "before": before + token, "from": rng.pick([2, 3])]]
        case 89..<91:
            let w = rng.pick(["id", "I'd", "ill", "I'll", "hell", "he'll", "well", "we'll", "its", "it's", "your", "you're", "there",
                              "their", "they're", "then", "than", "Its", "Your", "lets", "wed", "shell"])
            return [["op": "choice", "w": w, "prev": rng.chance(0.7) ? word(language) as Any : NSNull(), "next": rng.chance(0.6) ? word(language) as Any : NSNull(), "starts": rng.chance(0.3)]]
        case 91..<95: return [casingOp(token)]
        case 95..<97:
            let corrected = rng.chance(0.5) ? token : typo(token, language)
            let anchor = before + corrected
            return [["op": "record", "original": token, "corrected": corrected, "anchor": anchor,
                     "changes": [[token, corrected, rng.chance(0.7) ? language.rawValue as Any : NSNull()]]],
                    ["op": "revertible", "before": rng.chance(0.6) ? anchor : anchor + rng.pick([" ", "x", ""])]]
        default: return [staticOp(before: before, token: token)]
        }
    }

    mutating func mergeOps(before: String, set: KeySet) -> [[String: Any]] {
        let w = word(language)
        let chars = Array(w.lowercased())
        guard chars.count >= 5 else { return [] }
        let i = 2 + rng.int(chars.count - 3)
        let first = String(chars[..<i]), second = String(chars[(i + 1)...])
        guard let readFirst = taps(for: first, set, spread: 0.15),
              let readSecond = taps(for: second, set, spread: 0.15),
              let aim = set.keys.centres[chars[i]] ?? set.letters.first?.1 else { return [] }
        let (typedFirst, firstTaps) = readFirst
        let (typedSecond, secondTaps) = readSecond
        let space = CGPoint(x: Double(aim.x) + rng.gaussian() * 8,
                            y: Double(set.keys.bottomRowY) + Double(set.keys.pitch.height) * (0.3 + rng.uniform() * 0.8))
        return [
            ["op": "taps", "taps": Self.json(secondTaps)],
            ["op": "merge", "first": typedFirst, "ftaps": Self.json(firstTaps), "space": bits(space), "second": typedSecond,
             "before": before + typedFirst],
        ]
    }

    mutating func casingOp(_ token: String) -> [String: Any] {
        let names = ACParityWords.names.keys.sorted().flatMap { ACParityWords.names[$0]! }
        let w = rng.chance(0.5) ? rng.pick(names) : rng.chance(0.5) ? cased(word(language)) : rng.pick(ACParityWords.vocabulary)
        switch rng.int(7) {
        case 0: return ["op": "stray", "w": w, "typed": rng.chance(0.5) ? "" : token]
        case 1: return ["op": "added", "w": w, "typed": token.lowercased()]
        case 2: return ["op": "addedpool", "pool": [w, cased(word(language)), token, w.lowercased()], "typed": token.lowercased()]
        case 3: return ["op": "ownspelling", "w": w]
        case 4: return ["op": "ownwords"]
        case 5: return ["op": "vterm", "w": rng.chance(0.5) ? token : w.lowercased()]
        default: return ["op": "blocked"]
        }
    }

    mutating func staticOp(before: String, token: String) -> [String: Any] {
        let s = rng.chance(0.3) ? rng.pick(ACParityWords.tricky) : token
        switch rng.int(8) {
        case 0: return ["op": "token", "before": before + s]
        case 1: return ["op": "sentence", "before": before, "l": rng.pick([language, .greek, .english]).rawValue]
        case 2: return ["op": "prevword", "before": before + s, "token": rng.chance(0.8) ? s : ""]
        case 3: return ["op": "starts", "before": before + s, "token": s]
        case 4: return ["op": "matchcase", "typed": s, "target": rng.chance(0.5) ? word(language) : rng.pick(["I'm", "iPhone", "On my way!", "GitHub", "ß"])]
        case 5:
            let l = rng.pick(KeyboardLanguage.allCases)
            return ["op": "cost", "s": s.lowercased(), "t": rng.chance(0.5) ? typo(s.lowercased(), l) : word(language),
                    "l": l.rawValue, "letters": rng.chance(0.5) ? rng.pick(l.acoLetterLayouts).rawValue as Any : NSNull()]
        case 6: return ["op": "vowel", "w": s]
        default: return ["op": "vspell", "token": s, "term": rng.pick(ACParityWords.vocabulary)]
        }
    }
}
