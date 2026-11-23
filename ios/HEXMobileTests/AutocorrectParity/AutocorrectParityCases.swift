import CoreGraphics
import Foundation
@testable import HEX

/// Makes parity cases (TapParityRandom, a fixed seed per language): settings, your words, a
/// bilingual partner, Spatial autocorrect with the keys and taps, then questions the keyboard
/// asks with text before the cursor drawn from the language's words: typed right, slipped
/// (neighbouring keys, swaps, letters too many or too few, read from noisy taps), without
/// apostrophes or accents, in capitals, after punctuation, handles and numbers.
@MainActor
struct ACParityGenerator {
    struct KeySet {
        var stock: Int
        var personal: Int
        var keys: TapOracle.Keys
        var letters: [(Character, CGPoint)]
    }

    let language: KeyboardLanguage
    let dictionaries: [String: ACStubDictionary]
    let models: [String: TapOracleModel]
    var rng: TapParityRandom
    /// Keysets by "language/letters", shared by every generator (ids are global).
    var keysets: [String: KeySet]
    var keysetLines: [[String: Any]] = []
    var keysetsByID: [Int: TapOracle.Keys]
    var nextKeysetID: Int
    var partnerTags = 0

    static let available = ["en_US", "en", "es", "de", "it", "fr", "fr_CA", "fr_CH", "pt_BR", "pl", "cs", "hu", "ro", "ru", "uk", "el", "nb"]
    static let latin: [KeyboardLanguage] = [.english, .spanish, .german, .french, .frenchCanadian, .frenchSwiss, .italian, .portuguese, .polish, .czech, .hungarian, .norwegian, .romanian]

    // MARK: Words

    func words(_ l: KeyboardLanguage) -> [String] {
        dictionaries[ACStub.dictionaryCode(l.acoSpellCheckCandidates.first ?? l.rawValue)]?.ranked ?? []
    }

    mutating func word(_ l: KeyboardLanguage) -> String {
        let list = words(l)
        guard !list.isEmpty else { return "word" }
        let edges = [0, 200, 2000, list.count]
        let b = min(rng.int(3), 2)
        let lo = min(edges[b], list.count - 1), hi = max(lo + 1, min(edges[b + 1], list.count))
        return list[lo + rng.int(hi - lo)]
    }

    func rows(_ l: KeyboardLanguage) -> [[Character]] {
        l.letterRows(l.letterLayouts[0]).map { $0.compactMap(\.first) }
    }

    /// A key next to `c` on `l`'s own layout, or a random letter.
    mutating func neighbour(_ c: Character, _ l: KeyboardLanguage) -> Character {
        let coordinates = l.keyCoordinates()
        guard let p = coordinates[c] else { return rng.pick(Array("etaoinsrhl")) }
        let near = coordinates.filter { $0.key != c && hypot($0.value.0 - p.0, $0.value.1 - p.1) <= 1.6 }.map(\.key).sorted()
        return near.isEmpty ? c : rng.pick(near)
    }

    mutating func typo(_ w: String, _ l: KeyboardLanguage) -> String {
        var c = Array(w)
        guard c.count >= 2 else { return w + String(neighbour(c.first ?? "e", l)) }
        let i = rng.int(c.count)
        switch rng.int(5) {
        case 0: c[i] = neighbour(c[i], l)
        case 1: let j = min(i, c.count - 2); c.swapAt(j, j + 1)
        case 2: c.remove(at: i)
        case 3: c.insert(neighbour(c[i], l), at: i)
        default: c.insert(c[i], at: i)
        }
        return String(c)
    }

    mutating func cased(_ w: String) -> String {
        switch rng.int(20) {
        case 0, 1, 2: return w.prefix(1).uppercased() + w.dropFirst()
        case 3: return w.uppercased()
        case 4: return w.count > 2 ? String(w.prefix(1)) + w.dropFirst().prefix(1).uppercased() + w.dropFirst(2) : w
        default: return w
        }
    }

    func strippedAccents(_ w: String) -> String { w.folding(options: .diacriticInsensitive, locale: nil) }

    /// A word as it might be typed in `l`, before casing.
    mutating func rawToken(_ l: KeyboardLanguage, partner: KeyboardLanguage?) -> String {
        let roll = rng.int(100)
        switch roll {
        case 0..<38: return word(l)
        case 38..<62: return typo(word(l), l)
        case 62..<67:
            let fixes = ["dont", "im", "cant", "wont", "youre", "thats", "ive", "teh", "taht", "iphnoe", "i", "id", "ill", "well",
                         "its", "lets", "were", "hell", "shell", "wed", "your", "there", "their", "then", "than",
                         "i'm", "i'll", "i've", "i'd", "i\u{2019}m", "i\u{2019}ll", "l'hpmme", "k'homme", "qi'il", "jusqu'ici"]
            return rng.pick(fixes)
        case 67..<71:
            let accented = words(l).filter { strippedAccents($0) != $0 }
            if l == .italian, rng.chance(0.5) { return rng.pick(["perche", "piu", "citta", "caffe", "lunedi", "e", "ce", "cosi"]) }
            return accented.isEmpty ? word(l) : strippedAccents(rng.pick(accented))
        case 71..<74: return rng.pick(ACParityWords.lexicon).0.trimmingCharacters(in: .whitespaces)
        case 74..<77: return rng.pick(ACParityWords.vocabulary).split(separator: " ").first.map(String.init)!.lowercased()
        case 77..<80:
            let swears = ["fuck", "shit", "damn", "bitch", "cunt", "merde", "putain", "cazzo", "mierda", "scheiße", "faggot", "blow", "job"]
            return rng.pick(swears)
        case 80..<84:
            let names = ACParityWords.names[ACStub.dictionaryCode(l.acoSpellCheckCandidates[0])] ?? ["Mary"]
            let name = rng.pick(names)
            return rng.chance(0.6) ? name.lowercased() : name
        case 84..<87:
            let two = l.acoCommonTwoLetterWords.isEmpty ? ["to", "of"] : l.acoCommonTwoLetterWords
            return typo(rng.pick(two), l).prefix(2).lowercased()
        case 87..<92: return partner.map { word($0) } ?? word(.english)
        case 92..<95: return typo(word(l), l) + typo(word(l), l)
        case 95..<97: return rng.pick(ACParityWords.tricky)
        default: return rng.pick(["qzxv", "tkm", "dk", "dl", "xyzzy", "blorptastic", "Hexkey", "ok", "brb", "lol", "upto", "alot", "tb"])
        }
    }

    // MARK: Text before the cursor

    mutating func prefix(_ l: KeyboardLanguage, partner: KeyboardLanguage?) -> String {
        let roll = rng.int(100)
        if roll < 25 { return "" }
        if roll < 30 { return rng.pick(["@", "#", "http://x.com/", "foo_", "3", "e-", "re:", "a.", "(", "hi,", "\"", "x/"]) }
        var out = ""
        let n = 1 + rng.int(5)
        for i in 0..<n {
            let from = partner != nil && rng.chance(0.4) ? partner! : l
            var w = word(from)
            if i == 0, rng.chance(0.5) { w = w.prefix(1).uppercased() + w.dropFirst() }
            if rng.chance(0.08) { w = typo(w, from) }
            out += w
            let sep = rng.int(40)
            out += sep < 30 ? " " : sep < 33 ? ", " : sep < 35 ? ". " : sep < 36 ? "\n" : sep < 37 ? "? " : sep < 38 ? "  " : sep < 39 ? "; " : "! "
        }
        if roll < 33 { out += rng.pick(["(", "\"", "'", "-", "/", "@"]) }
        return out
    }

    // MARK: Keys and taps

    mutating func keyset(_ l: KeyboardLanguage, _ letters: KeyboardLetterLayout) -> KeySet? {
        let name = "\(l.rawValue)/\(letters.rawValue)"
        if let k = keysets[name] { return k }
        let layout = KeyboardLayout(width: 390, language: l, letters: letters)
        guard let stock = TapOracle.Keys(layout: layout) else { return nil }
        var offsets: [Character: CGVector] = [:]
        for c in stock.centres.keys.sorted() {
            offsets[c] = CGVector(dx: rng.gaussian() * 4, dy: rng.gaussian() * 5)
        }
        guard let personal = TapOracle.Keys(layout: layout, offsets: offsets) else { return nil }
        let set = KeySet(stock: nextKeysetID, personal: nextKeysetID + 1, keys: stock, letters: stock.centres.sorted { $0.key < $1.key }.map { ($0.key, $0.value) })
        for (id, keys) in [(set.stock, stock), (set.personal, personal)] {
            keysetLines.append(["kind": "keys", "id": id, "keys": ACParityGenerator.json(keys)])
            keysetsByID[id] = keys
        }
        nextKeysetID += 2
        keysets[name] = set
        return set
    }

    /// Noisy taps aimed at `word`'s letters, and the letters the nearest keys typed.
    mutating func taps(for word: String, _ set: KeySet, spread: Double) -> (String, [CGPoint])? {
        var taps: [CGPoint] = []
        for c in word.lowercased() where c != "'" {
            let p = set.keys.centres[c] ?? TapOracle.withoutAccents(String(c)).first.flatMap { set.keys.centres[$0] }
            guard let p else { return nil }
            taps.append(CGPoint(x: Double(p.x) + rng.gaussian() * spread * Double(set.keys.pitch.width),
                                y: Double(p.y) + rng.gaussian() * spread * Double(set.keys.pitch.height)))
        }
        guard !taps.isEmpty else { return nil }
        let pw = Double(set.keys.pitch.width), ph = Double(set.keys.pitch.height)
        let typed = String(taps.map { t -> Character in
            var best = set.letters[0].0, distance = Double.infinity
            for (c, p) in set.letters {
                let dx = (Double(t.x) - Double(p.x)) / pw, dy = (Double(t.y) - Double(p.y)) / ph
                if dx * dx + dy * dy < distance { distance = dx * dx + dy * dy; best = c }
            }
            return best
        })
        return (typed, taps)
    }

    static func json(_ taps: [CGPoint]) -> [[String]] { taps.map(bits) }

    static func json(_ keys: TapOracle.Keys) -> [String: Any] {
        [
            "centres": keys.centres.sorted { $0.key < $1.key }.map { [String($0.key)] + bits($0.value) },
            "pitch": [bits(Double(keys.pitch.width)), bits(Double(keys.pitch.height))],
            "bottom": bits(Double(keys.bottomRowY)),
            "space": keys.space.map { [$0.origin.x, $0.origin.y, $0.size.width, $0.size.height].map { bits(Double($0)) } } ?? NSNull(),
        ]
    }

    /// The letter grid the host shows for `language` on `letters`, by name ("fr-CA/qwerty"):
    /// the fixture writes each grid once (`gridLine`).
    static func grid(_ language: KeyboardLanguage, _ letters: KeyboardLetterLayout?) -> String {
        language.rawValue + "/" + (letters?.rawValue ?? "-")
    }

    /// A grid's line: the layout's language and (letter, row, column) by letter
    /// (`keyCoordinates`).
    static func gridLine(_ ref: String) -> [String: Any] {
        let parts = ref.split(separator: "/").map(String.init)
        let language = KeyboardLanguage(rawValue: parts[0])!
        let letters = KeyboardLetterLayout(rawValue: parts[1])
        let cells = language.keyCoordinates(letters).sorted { $0.key < $1.key }.map { [String($0.key), bits($0.value.0), bits($0.value.1)] }
        return ["kind": "grid", "ref": ref, "l": language.rawValue, "cells": cells]
    }
}
