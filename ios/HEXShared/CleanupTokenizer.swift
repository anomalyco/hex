import Foundation

/// The cleanup tagger's BERT uncased WordPiece tokenizer, matching Hugging Face's
/// `BertTokenizer` as the tagger was trained and exported with it (`tokenizer.json`):
/// special tokens split out first, then `BertNormalizer` (clean text, space around CJK
/// ideographs, NFD accent stripping, lowercasing), `BertPreTokenizer` (whitespace and
/// punctuation), and greedy longest-match WordPiece with `##` continuations.
///
/// Hugging Face's character classes come from Unicode 8.0 tables, so punctuation,
/// nonspacing marks, and control or format characters assigned later are treated as
/// ordinary characters, exactly as the trained model saw them.
nonisolated struct CleanupTokenizer: Sendable {
    static let padID: Int32 = 0
    static let unknownID: Int32 = 100
    static let clsID: Int32 = 101
    static let sepID: Int32 = 102
    static let maxCharactersPerWord = 100

    enum LoadError: Error {
        case wrongSize(Int)
    }

    private static let specialTokens: [(scalars: [Unicode.Scalar], id: Int32)] = [
        ("[PAD]", 0), ("[UNK]", 100), ("[CLS]", 101), ("[SEP]", 102), ("[MASK]", 103),
    ].map { (Array($0.0.unicodeScalars), $0.1) }
    private static let tableVersion = Unicode.Version(major: 8, minor: 0)
    private static let continuation: [UInt8] = Array("##".utf8)

    /// Keyed by exact UTF-8 bytes: Swift's `String` equality is canonical equivalence,
    /// which would match a precomposed vocabulary entry the Rust tokenizer never matches.
    private let vocabulary: [[UInt8]: Int32]

    /// `text` is `vocab.txt`: one token per line, the line number is its id.
    init(vocabulary text: String, expectedSize: Int? = 30522) throws {
        var vocabulary: [[UInt8]: Int32] = [:]
        vocabulary.reserveCapacity(expectedSize ?? 32_000)
        var lines = text.utf8.split(separator: UInt8(ascii: "\n"), omittingEmptySubsequences: false)
        if lines.last?.isEmpty == true {
            lines.removeLast()
        }
        for (id, line) in lines.enumerated() where vocabulary[Array(line)] == nil {
            vocabulary[Array(line)] = Int32(id)
        }
        if let expectedSize, lines.count != expectedSize {
            throw LoadError.wrongSize(lines.count)
        }
        self.vocabulary = vocabulary
    }

    /// WordPiece ids for one whitespace-separated word, without [CLS] or [SEP].
    func encode(_ word: String) -> [Int32] {
        var ids: [Int32] = []
        let scalars = Array(word.unicodeScalars)
        var text: [Unicode.Scalar] = []
        var index = 0
        while index < scalars.count {
            if scalars[index] == "[", let special = Self.specialToken(in: scalars, at: index) {
                appendText(text, to: &ids)
                text.removeAll()
                ids.append(special.id)
                index += special.length
            } else {
                text.append(scalars[index])
                index += 1
            }
        }
        appendText(text, to: &ids)
        return ids
    }

    private func appendText(_ text: [Unicode.Scalar], to ids: inout [Int32]) {
        guard !text.isEmpty else { return }
        for piece in Self.preTokenize(Self.normalize(text)) {
            ids += wordPiece(piece)
        }
    }

    private static func specialToken(in scalars: [Unicode.Scalar], at index: Int) -> (id: Int32, length: Int)? {
        for token in specialTokens where index + token.scalars.count <= scalars.count {
            if scalars[index..<(index + token.scalars.count)].elementsEqual(token.scalars) {
                return (token.id, token.scalars.count)
            }
        }
        return nil
    }

    // MARK: BertNormalizer

    static func normalize(_ scalars: [Unicode.Scalar]) -> [Unicode.Scalar] {
        var cleaned = String.UnicodeScalarView()
        for scalar in scalars {
            if scalar.value == 0 || scalar.value == 0xFFFD || isControl(scalar) {
                continue
            }
            if isWhitespace(scalar) {
                cleaned.append(" ")
            } else if isChineseCharacter(scalar) {
                cleaned.append(contentsOf: [" ", scalar, " "])
            } else {
                cleaned.append(scalar)
            }
        }
        var normalized: [Unicode.Scalar] = []
        normalized.reserveCapacity(cleaned.count)
        for scalar in String(cleaned).decomposedStringWithCanonicalMapping.unicodeScalars
        where !isNonspacingMark(scalar) {
            normalized.append(contentsOf: scalar.properties.lowercaseMapping.unicodeScalars)
        }
        return normalized
    }

    private static func isWhitespace(_ scalar: Unicode.Scalar) -> Bool {
        scalar == "\t" || scalar == "\n" || scalar == "\r" || scalar.properties.isWhitespace
    }

    private static func isControl(_ scalar: Unicode.Scalar) -> Bool {
        if scalar == "\t" || scalar == "\n" || scalar == "\r" { return false }
        switch scalar.properties.generalCategory {
        case .control, .format, .privateUse, .surrogate:
            return inTables(scalar)
        default:
            return false
        }
    }

    private static func isNonspacingMark(_ scalar: Unicode.Scalar) -> Bool {
        scalar.properties.generalCategory == .nonspacingMark && inTables(scalar)
    }

    private static func isPunctuation(_ scalar: Unicode.Scalar) -> Bool {
        if scalar.isASCII {
            let value = scalar.value
            return (0x21...0x2F).contains(value) || (0x3A...0x40).contains(value)
                || (0x5B...0x60).contains(value) || (0x7B...0x7E).contains(value)
        }
        switch scalar.properties.generalCategory {
        case .connectorPunctuation, .dashPunctuation, .openPunctuation, .closePunctuation,
             .initialPunctuation, .finalPunctuation, .otherPunctuation:
            return inTables(scalar)
        default:
            return false
        }
    }

    /// Hugging Face's category tables stop at Unicode 8.0.
    private static func inTables(_ scalar: Unicode.Scalar) -> Bool {
        guard let age = scalar.properties.age else { return false }
        return age.major < tableVersion.major || (age.major == tableVersion.major && age.minor <= tableVersion.minor)
    }

    private static func isChineseCharacter(_ scalar: Unicode.Scalar) -> Bool {
        switch scalar.value {
        case 0x4E00...0x9FFF, 0x3400...0x4DBF, 0x20000...0x2A6DF, 0x2A700...0x2B73F,
             0x2B740...0x2B81F, 0x2B920...0x2CEAF, 0xF900...0xFAFF, 0x2F800...0x2FA1F:
            return true
        default:
            return false
        }
    }

    // MARK: BertPreTokenizer

    static func preTokenize(_ scalars: [Unicode.Scalar]) -> [[Unicode.Scalar]] {
        var pieces: [[Unicode.Scalar]] = []
        var current: [Unicode.Scalar] = []
        for scalar in scalars {
            if scalar.properties.isWhitespace {
                if !current.isEmpty { pieces.append(current) }
                current.removeAll()
            } else if isPunctuation(scalar) {
                if !current.isEmpty { pieces.append(current) }
                current.removeAll()
                pieces.append([scalar])
            } else {
                current.append(scalar)
            }
        }
        if !current.isEmpty { pieces.append(current) }
        return pieces
    }

    // MARK: WordPiece

    private func wordPiece(_ piece: [Unicode.Scalar]) -> [Int32] {
        guard piece.count <= Self.maxCharactersPerWord else { return [Self.unknownID] }
        let bytes = piece.map { Array(String($0).utf8) }
        var ids: [Int32] = []
        var start = 0
        while start < piece.count {
            var end = piece.count
            var match: Int32?
            while start < end {
                var key = start > 0 ? Self.continuation : []
                for index in start..<end {
                    key += bytes[index]
                }
                if let id = vocabulary[key] {
                    match = id
                    break
                }
                end -= 1
            }
            guard let match else { return [Self.unknownID] }
            ids.append(match)
            start = end
        }
        return ids
    }
}
