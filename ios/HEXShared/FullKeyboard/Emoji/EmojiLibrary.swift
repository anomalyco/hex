import CoreText
import Foundation

/// The emoji panel's categories, in the order its bar shows them.
nonisolated enum EmojiCategory: Int, CaseIterable, Sendable {
    case smileysPeople
    case animalsNature
    case foodDrink
    case activity
    case travelPlaces
    case objects
    case symbols
    case flags

    var title: String {
        switch self {
        case .smileysPeople: "Smileys and people"
        case .animalsNature: "Animals and nature"
        case .foodDrink: "Food and drink"
        case .activity: "Activity"
        case .travelPlaces: "Travel and places"
        case .objects: "Objects"
        case .symbols: "Symbols"
        case .flags: "Flags"
        }
    }

    /// The category bar's SF Symbol.
    var symbol: String {
        switch self {
        case .smileysPeople: "face.smiling.inverse"
        case .animalsNature: "teddybear.fill"
        case .foodDrink: "takeoutbag.and.cup.and.straw.fill"
        case .activity: "soccerball.inverse"
        case .travelPlaces: "building.2.fill"
        case .objects: "lightbulb.fill"
        case .symbols: "music.note"
        case .flags: "flag.fill"
        }
    }
}

/// One emoji the panel offers, with its skin tones (light to dark) when it has them.
nonisolated struct EmojiEntry: Equatable, Sendable {
    let emoji: String
    let category: EmojiCategory
    let tones: [String]

    var hasTones: Bool { !tones.isEmpty }
}

/// The emoji catalog and its keyword index for one keyboard layout, read from
/// `emoji-index-<lang>.txt` (built by scripts/build-emoji-data.py from Unicode's emoji-test.txt
/// and CLDR, Unicode License v3, and Hex's own chat words).
///
/// The file is memory-mapped and searched in place: names, keywords and postings stay in
/// its clean pages, which iOS doesn't count against the keyboard's memory limit, and the
/// heap holds only the emoji and a few offset arrays. Immutable once loaded.
nonisolated final class EmojiLibrary: @unchecked Sendable {
    struct Hit: Equatable, Sendable {
        let entry: Int32
        let weight: Float
    }

    let entries: [EmojiEntry]
    let language: String
    /// Words too common to say anything about an emoji, in the layout's language and English.
    let stopwords: Set<String>
    /// Every emoji and skin tone variant to its entry.
    let entryByEmoji: [String: Int]
    /// The longest phrase indexed, in words.
    let longestPhrase: Int

    private let data: Data
    private let families: [Int32]
    /// Each entry's name, then its search words (name words, "|", keyword words), as byte
    /// offsets into `data`.
    private let nameStart: [Int32]
    private let searchStart: [Int32]
    private let searchSplit: [Int32]
    private let searchEnd: [Int32]
    /// Where each word's and phrase's line starts (its key), sorted by key.
    private let wordLines: [Int32]
    private let phraseLines: [Int32]
    /// The file's entry numbers to ours; -1 for emoji this iOS doesn't draw.
    private let remap: [Int32]

    private init(
        data: Data,
        language: String,
        entries: [EmojiEntry],
        stopwords: Set<String>,
        families: [Int32],
        nameStart: [Int32],
        searchStart: [Int32],
        searchSplit: [Int32],
        searchEnd: [Int32],
        wordLines: [Int32],
        phraseLines: [Int32],
        longestPhrase: Int,
        remap: [Int32]
    ) {
        self.data = data
        self.remap = remap
        self.language = language
        self.entries = entries
        self.stopwords = stopwords
        self.families = families
        self.nameStart = nameStart
        self.searchStart = searchStart
        self.searchSplit = searchSplit
        self.searchEnd = searchEnd
        self.wordLines = wordLines
        self.phraseLines = phraseLines
        self.longestPhrase = longestPhrase
        var byEmoji: [String: Int] = [:]
        byEmoji.reserveCapacity(entries.count * 2)
        for (index, entry) in entries.enumerated() {
            byEmoji[entry.emoji] = index
            for tone in entry.tones { byEmoji[tone] = index }
        }
        entryByEmoji = byEmoji
    }

    // MARK: - Loading

    /// The layouts with an index; any other language reads English's.
    static let languages = ["en", "nb", "es", "de"]

    /// The newest emoji version every supported iOS draws; newer ones are listed only if
    /// the emoji font draws them.
    static let assumedVersion = 15.1

    /// Loads the index for a keyboard layout ("en", "nb", "es", "de"), whose words come
    /// with English's.
    static func load(
        language: String,
        bundle: Bundle = .main,
        isDrawable: (String) -> Bool = EmojiLibrary.fontDraws
    ) -> EmojiLibrary? {
        let language = languages.contains(language) ? language : "en"
        guard let url = bundle.url(forResource: "emoji-index-\(language)", withExtension: "txt"),
              let data = try? Data(contentsOf: url, options: .alwaysMapped) else { return nil }
        return load(data: data, language: language, isDrawable: isDrawable)
    }

    static func load(data: Data, language: String, isDrawable: (String) -> Bool = EmojiLibrary.fontDraws) -> EmojiLibrary? {
        var entries: [EmojiEntry] = []
        var families: [Int32] = []
        var nameStart: [Int32] = []
        var searchStart: [Int32] = []
        var searchSplit: [Int32] = []
        var searchEnd: [Int32] = []
        var wordLines: [Int32] = []
        var phraseLines: [Int32] = []
        var stopwords: Set<String> = []
        var longestPhrase = 1
        // Index files number entries by line; an emoji this iOS can't draw keeps its number
        // and is mapped out.
        var remap: [Int32] = []
        entries.reserveCapacity(2_000)
        remap.reserveCapacity(2_000)

        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let bytes = raw.bindMemory(to: UInt8.self)
            let count = bytes.count
            var lineStart = 0
            while lineStart < count {
                var lineEnd = lineStart
                while lineEnd < count, bytes[lineEnd] != 10 { lineEnd += 1 }
                defer { lineStart = lineEnd + 1 }
                guard lineEnd - lineStart > 2, bytes[lineStart + 1] == 9 else { continue }
                let body = lineStart + 2
                switch bytes[lineStart] {
                case UInt8(ascii: "W"):
                    wordLines.append(Int32(body))
                case UInt8(ascii: "P"):
                    phraseLines.append(Int32(body))
                    var spaces = 1
                    var i = body
                    while i < lineEnd, bytes[i] != 9 {
                        if bytes[i] == 32 { spaces += 1 }
                        i += 1
                    }
                    longestPhrase = max(longestPhrase, spaces)
                case UInt8(ascii: "S"):
                    let text = String(decoding: UnsafeBufferPointer(rebasing: bytes[body..<lineEnd]), as: UTF8.self)
                    stopwords = Set(text.split(separator: " ").map(String.init))
                case UInt8(ascii: "E"):
                    // emoji, category, version, tones, family, name, search words
                    var fields: [Range<Int>] = []
                    var start = body
                    for i in body...lineEnd where i == lineEnd || bytes[i] == 9 {
                        fields.append(start..<i)
                        start = i + 1
                    }
                    guard fields.count == 7 else { continue }
                    func string(_ range: Range<Int>) -> String {
                        String(decoding: UnsafeBufferPointer(rebasing: bytes[range]), as: UTF8.self)
                    }
                    let emoji = string(fields[0])
                    let version = Double(string(fields[2])) ?? 0
                    guard version <= assumedVersion || isDrawable(emoji),
                          let raw = Int(string(fields[1])),
                          let category = EmojiCategory(rawValue: raw) else {
                        remap.append(-1)
                        continue
                    }
                    let tones = fields[3].isEmpty ? [] : string(fields[3]).split(separator: " ").map(String.init)
                    remap.append(Int32(entries.count))
                    entries.append(EmojiEntry(emoji: emoji, category: category, tones: tones))
                    families.append(Int32(string(fields[4])) ?? -1)
                    nameStart.append(Int32(fields[5].lowerBound))
                    let search = fields[6]
                    var split = search.upperBound
                    for i in search where bytes[i] == UInt8(ascii: "|") {
                        split = i
                        break
                    }
                    searchStart.append(Int32(search.lowerBound))
                    searchSplit.append(Int32(split))
                    searchEnd.append(Int32(search.upperBound))
                default:
                    break
                }
            }
        }
        guard !entries.isEmpty else { return nil }
        return EmojiLibrary(
            data: data,
            language: language,
            entries: entries,
            stopwords: stopwords,
            families: families,
            nameStart: nameStart,
            searchStart: searchStart,
            searchSplit: searchSplit,
            searchEnd: searchEnd,
            wordLines: wordLines,
            phraseLines: phraseLines,
            longestPhrase: longestPhrase,
            remap: remap
        )
    }

    /// Whether the system emoji font itself draws `emoji` as one glyph: a sequence it doesn't
    /// know falls apart into several, and an emoji it lacks falls back to another font's box.
    static func fontDraws(_ emoji: String) -> Bool {
        let font = CTFontCreateWithName("AppleColorEmoji" as CFString, 20, nil)
        let attributed = NSAttributedString(
            string: emoji,
            attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font]
        )
        let line = CTLineCreateWithAttributedString(attributed)
        guard CTLineGetGlyphCount(line) == 1,
              let run = (CTLineGetGlyphRuns(line) as? [CTRun])?.first else { return false }
        let attributes = CTRunGetAttributes(run) as NSDictionary
        guard let used = attributes[kCTFontAttributeName as String] else { return false }
        let name = CTFontCopyPostScriptName(used as! CTFont) as String
        var glyph = CGGlyph()
        CTRunGetGlyphs(run, CFRange(location: 0, length: 1), &glyph)
        return glyph != 0 && name == "AppleColorEmoji"
    }

    // MARK: - Lookup

    /// The emoji a folded word names or describes, strongest first; nil when none.
    func hits(forWord word: String) -> [Hit]? {
        lookup(word, in: wordLines)
    }

    func hits(forPhrase phrase: String) -> [Hit]? {
        lookup(phrase, in: phraseLines)
    }

    private func lookup(_ key: String, in lines: [Int32]) -> [Hit]? {
        var key = key
        return key.withUTF8 { needle -> [Hit]? in
            data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) -> [Hit]? in
                let bytes = raw.bindMemory(to: UInt8.self)
                var low = 0
                var high = lines.count - 1
                while low <= high {
                    let mid = (low + high) / 2
                    let start = Int(lines[mid])
                    let order = Self.compare(needle, bytes, at: start)
                    if order == 0 {
                        return postings(bytes, from: start + needle.count + 1)
                    }
                    if order < 0 { high = mid - 1 } else { low = mid + 1 }
                }
                return nil
            }
        }
    }

    /// Compares `needle` with the tab-ended key at `start`, by bytes (the file sorts keys by
    /// code point, which UTF-8 keeps).
    private static func compare(_ needle: UnsafeBufferPointer<UInt8>, _ bytes: UnsafeBufferPointer<UInt8>, at start: Int) -> Int {
        var i = 0
        while true {
            let k = start + i < bytes.count ? bytes[start + i] : 9
            let keyEnded = k == 9
            let needleEnded = i == needle.count
            if keyEnded && needleEnded { return 0 }
            if needleEnded { return -1 }
            if keyEnded { return 1 }
            if needle[i] != k { return needle[i] < k ? -1 : 1 }
            i += 1
        }
    }

    /// Reads "12:0.500,40:0.375" up to the line's end.
    private func postings(_ bytes: UnsafeBufferPointer<UInt8>, from start: Int) -> [Hit] {
        var hits: [Hit] = []
        var i = start
        var entry = 0
        var weight: Float = 0
        var scale: Float = 0
        var inWeight = false
        func flush() {
            if entry < remap.count, remap[entry] >= 0 {
                hits.append(Hit(entry: remap[entry], weight: weight))
            }
            entry = 0
            weight = 0
            scale = 0
            inWeight = false
        }
        while i < bytes.count, bytes[i] != 10 {
            let byte = bytes[i]
            switch byte {
            case UInt8(ascii: "0")...UInt8(ascii: "9"):
                let digit = Float(byte - UInt8(ascii: "0"))
                if !inWeight {
                    entry = entry * 10 + Int(digit)
                } else if scale == 0 {
                    weight = weight * 10 + digit
                } else {
                    weight += digit * scale
                    scale /= 10
                }
            case UInt8(ascii: ":"):
                inWeight = true
            case UInt8(ascii: "."):
                scale = 0.1
            case UInt8(ascii: ","):
                flush()
            default:
                break
            }
            i += 1
        }
        if inWeight { flush() }
        return hits
    }

    /// Every single word the index knows, for the semantic index's vocabulary.
    var vocabulary: [String] {
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let bytes = raw.bindMemory(to: UInt8.self)
            return wordLines.map { line in
                var end = Int(line)
                while end < bytes.count, bytes[end] != 9 { end += 1 }
                return String(decoding: UnsafeBufferPointer(rebasing: bytes[Int(line)..<end]), as: UTF8.self)
            }
        }
    }

    /// The emoji's English name ("party popper"), for VoiceOver.
    func name(of index: Int) -> String {
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let bytes = raw.bindMemory(to: UInt8.self)
            let start = Int(nameStart[index])
            var end = start
            while end < bytes.count, bytes[end] != 9 { end += 1 }
            return String(decoding: UnsafeBufferPointer(rebasing: bytes[start..<end]), as: UTF8.self)
        }
    }

    /// Emoji whose names differ only by who does it ("man running", "woman running",
    /// "person running facing right") share a family, so a suggestion row shows one of them.
    func family(of index: Int) -> Int32 {
        families[index]
    }

    /// The emoji a search finds: each of its words must start a word of the emoji's name or
    /// keywords. Whole name words first, then name prefixes, then keywords, each in catalog
    /// order, with `boost` (your use, 0...1) between equals.
    func search(_ query: String, limit: Int = 80, boost: (Int) -> Float = { _ in 0 }) -> [Int] {
        let terms = EmojiText.words(in: query)
        guard !terms.isEmpty else { return [] }
        var termBytes = terms.map { Array($0.utf8) }
        termBytes.sort { $0.count > $1.count }
        var scored: [(Int, Float)] = []
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            let bytes = raw.bindMemory(to: UInt8.self)
            for index in entries.indices {
                let start = Int(searchStart[index])
                let split = Int(searchSplit[index])
                let end = Int(searchEnd[index])
                var total: Float = 0
                var matchedAll = true
                for term in termBytes {
                    var best: Float = 0
                    var wordStart = start
                    while wordStart < end {
                        var wordEnd = wordStart
                        while wordEnd < end, bytes[wordEnd] != 32, bytes[wordEnd] != UInt8(ascii: "|") { wordEnd += 1 }
                        let length = wordEnd - wordStart
                        if length >= term.count {
                            var prefix = true
                            for k in 0..<term.count where bytes[wordStart + k] != term[k] {
                                prefix = false
                                break
                            }
                            if prefix {
                                let quality: Float = (length == term.count ? 2 : 1) + (wordStart < split ? 2 : 0)
                                best = max(best, quality)
                            }
                        }
                        if best == 4 { break }
                        wordStart = wordEnd + 1
                    }
                    if best == 0 {
                        matchedAll = false
                        break
                    }
                    total += best
                }
                if matchedAll {
                    // Among equals, a short name the query fills ("red heart") beats a long one
                    // it only touches ("smiling face with heart-eyes"), then your use.
                    var nameWords = 1
                    for i in start..<split where bytes[i] == 32 { nameWords += 1 }
                    let fill = 0.3 * Float(min(terms.count, nameWords)) / Float(nameWords)
                    scored.append((index, total + fill + 0.6 * min(1, boost(index))))
                }
            }
        }
        scored.sort { $0.1 != $1.1 ? $0.1 > $1.1 : $0.0 < $1.0 }
        return scored.prefix(limit).map(\.0)
    }
}

/// Folding and word splitting shared by the emoji index (scripts/build-emoji-data.py folds
/// the same way) and the context reader.
nonisolated enum EmojiText {
    /// NFD, combining marks dropped, lowercased, curly apostrophes straightened.
    static func fold(_ text: String) -> String {
        var scalars = String.UnicodeScalarView()
        for scalar in text.replacingOccurrences(of: "\u{2019}", with: "'").decomposedStringWithCanonicalMapping.unicodeScalars
        where scalar.properties.canonicalCombiningClass.rawValue == 0 {
            scalars.append(scalar)
        }
        return String(scalars).lowercased()
    }

    /// Folded words: letters and digits, an apostrophe's tail dropped ("that's" → "that").
    static func words(in text: String) -> [String] {
        fold(text)
            .split(whereSeparator: { !($0.isLetter || $0.isNumber || $0 == "'") })
            .compactMap { part -> String? in
                let head = part.split(separator: "'", maxSplits: 1).first.map(String.init) ?? ""
                return head.isEmpty ? nil : head
            }
    }

    /// A word with a letter held down for emphasis ("soooo", "lmaooo", "goooood"), with
    /// each run of three or more cut to two and to one: ["soo", "so"]. Empty otherwise.
    static func collapsingRepeats(_ word: String) -> [String] {
        var runs: [(Character, Int)] = []
        for character in word {
            if let last = runs.last, last.0 == character {
                runs[runs.count - 1].1 += 1
            } else {
                runs.append((character, 1))
            }
        }
        guard runs.contains(where: { $0.1 >= 3 }) else { return [] }
        func joined(_ cut: Int) -> String {
            String(runs.flatMap { Array(repeating: $0.0, count: $0.1 >= 3 ? cut : $0.1) })
        }
        return [joined(2), joined(1)]
    }
}
