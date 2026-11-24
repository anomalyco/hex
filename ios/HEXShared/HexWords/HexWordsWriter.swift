import Foundation

/// Writes the iPhone's Hex words file: Custom vocabulary's terms (no spellings), Word
/// replacements, and optionally the learned dictation fixes in use, marked `learned`. It leaves
/// out every entry a reader would skip, by the reader's own rules in the reader's order, and
/// says which and why, so it never writes a file its own reader would skip anything in.
nonisolated enum HexWordsWriter {
    static let fileName = "hex-words.hexwords"
    static let appName = "Hex for iPhone"

    /// A learned dictation fix; only one in use (applied and not switched off) is exported.
    struct LearnedFix: Equatable, Sendable {
        var heard: String
        var meant: String
        var active: Bool
    }

    struct Output: Equatable, Sendable {
        var data: Data
        var terms: Int
        var replacements: Int
        /// Paths in the app's own lists: `vocabulary[i]`, `replacements[i]`, `learnedFixes[i]`.
        var notExported: [HexWordsSkip]
    }

    static func phone(
        vocabulary: [String],
        replacements: [WordReplacement],
        learnedFixes: [LearnedFix] = [],
        includeLearned: Bool = false,
        appVersion: String
    ) -> Output {
        var notExported: [HexWordsSkip] = []
        var terms: [String] = []
        var termKeys: Set<String> = []
        for (index, raw) in vocabulary.enumerated() {
            let path = "vocabulary[\(index)]"
            guard terms.count < HexWordsReader.Limit.terms else {
                notExported.append(HexWordsSkip(path: path, reason: .overLimit))
                continue
            }
            let term = HexWordsText.cleanLine(raw)
            if let problem = HexWordsReader.termProblem(term) {
                notExported.append(HexWordsSkip(path: path, reason: problem))
                continue
            }
            guard termKeys.insert(HexWordsText.key(term)).inserted else {
                notExported.append(HexWordsSkip(path: path, reason: .duplicate))
                continue
            }
            terms.append(term)
        }

        var sources = replacements.enumerated().map { ("replacements[\($0.offset)]", $0.element.heard, $0.element.write, false) }
        if includeLearned {
            for (index, fix) in learnedFixes.enumerated() where fix.active {
                sources.append(("learnedFixes[\(index)]", fix.heard, fix.meant, true))
            }
        }
        var written: [HexWordsReplacement] = []
        var heardKeys: Set<String> = []
        for (path, rawHeard, rawWrite, learned) in sources {
            guard written.count < HexWordsReader.Limit.replacements else {
                notExported.append(HexWordsSkip(path: path, reason: .overLimit))
                continue
            }
            let heard = HexWordsText.cleanLine(rawHeard)
            let write = HexWordsText.cleanWrite(rawWrite)
            if let problem = HexWordsReader.replacementProblem(heard: heard, write: write) {
                notExported.append(HexWordsSkip(path: path, reason: problem))
                continue
            }
            guard heardKeys.insert(HexWordsText.key(heard)).inserted else {
                notExported.append(HexWordsSkip(path: path, reason: .duplicate))
                continue
            }
            written.append(HexWordsReplacement(heard: heard, write: write, learned: learned))
        }

        return Output(
            data: Data(json(terms: terms, replacements: written, appVersion: appVersion).utf8),
            terms: terms.count,
            replacements: written.count,
            notExported: notExported
        )
    }

    /// The running app's version, `CFBundleShortVersionString` (Hex for iPhone's
    /// `MARKETING_VERSION`), for `madeBy.version`.
    static func appVersion(bundle: Bundle = .main) -> String {
        bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
    }

    /// Writes `data` to a private temporary folder, atomically (write, then rename).
    static func temporaryFile(_ data: Data) throws -> URL {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("HexWordsExport-" + UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appendingPathComponent(fileName)
        try data.write(to: url, options: [.atomic, .completeFileProtection])
        return url
    }

    /// One object in the spec's order, UTF-8 without a byte order mark, non-ASCII as itself.
    private static func json(terms: [String], replacements: [HexWordsReplacement], appVersion: String) -> String {
        var madeBy = "{\"app\": \(quote(appName))"
        let version = HexWordsText.cleanLine(appVersion)
        if !version.isEmpty, !HexWordsText.hasHidden(version), HexWordsText.length(version) <= HexWordsReader.Limit.appVersionLength {
            madeBy += ", \"version\": \(quote(version))"
        }
        madeBy += "}"
        let vocabulary = terms.map { "    {\"term\": \(quote($0))}" }
        let lines = replacements.map { entry in
            "    {\"heard\": \(quote(entry.heard)), \"write\": \(quote(entry.write))" + (entry.learned ? ", \"learned\": true}" : "}")
        }
        return """
        {
          "format": "\(HexWordsReader.format)",
          "version": \(HexWordsReader.version),
          "madeBy": \(madeBy),
          "vocabulary": \(list(vocabulary)),
          "replacements": \(list(lines))
        }

        """
    }

    private static func list(_ items: [String]) -> String {
        items.isEmpty ? "[]" : "[\n" + items.joined(separator: ",\n") + "\n  ]"
    }

    private static func quote(_ text: String) -> String {
        var out = "\""
        for scalar in text.unicodeScalars {
            switch scalar {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\t": out += "\\t"
            case "\r": out += "\\r"
            default:
                if scalar.value < 0x20 {
                    out += String(format: "\\u%04x", scalar.value)
                } else {
                    out.unicodeScalars.append(scalar)
                }
            }
        }
        return out + "\""
    }
}
