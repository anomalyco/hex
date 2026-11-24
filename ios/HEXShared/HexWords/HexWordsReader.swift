import CoreFoundation
import Foundation

/// Why a file was refused; nothing of it is kept or shown entry by entry.
nonisolated enum HexWordsRefusal: String, Sendable, CaseIterable {
    case tooLarge, notUTF8, empty, tooDeep, duplicateKey, notJSON, notAnObject, notHexWords, badVersion
}

/// Why one part of a file that was read was left out.
nonisolated enum HexWordsSkipReason: String, Sendable, CaseIterable {
    case badPart, badEntry, badField, unknownField, hiddenCharacters, noLetters, tooLong, tooManyWords,
         duplicate, sameAsTerm, overLimit
}

/// A place in a file (`vocabulary[3].soundsLike[0]`) and why it was skipped.
nonisolated struct HexWordsSkip: Hashable, Sendable {
    var path: String
    var reason: HexWordsSkipReason
}

nonisolated struct HexWordsTerm: Equatable, Sendable {
    var term: String
    var soundsLike: [String] = []
}

nonisolated struct HexWordsReplacement: Equatable, Sendable {
    var heard: String
    var write: String
    var learned = false
}

/// A file that was read: what it holds, cleaned, and what was skipped.
nonisolated struct HexWordsFile: Equatable, Sendable {
    var version = 1
    /// `madeBy`: the file's claim, shown, never trusted.
    var madeByApp: String?
    var madeByVersion: String?
    var vocabulary: [HexWordsTerm] = []
    var replacements: [HexWordsReplacement] = []
    var skipped: [HexWordsSkip] = []

    var isNewerVersion: Bool { version > HexWordsReader.version }
}

nonisolated enum HexWordsReadResult: Equatable, Sendable {
    case refused(HexWordsRefusal)
    case read(HexWordsFile)
}

/// Reads a Hex words file (`.hexwords`) exactly as `docs/specs/hex-words.md` says: refuse a file
/// that isn't one, otherwise read what it can and skip each entry that breaks a rule.
nonisolated enum HexWordsReader {
    static let version = 1
    static let format = "hex.words"
    static let maxBytes = 8 * 1024 * 1024

    enum Limit {
        static let terms = 500
        static let termLength = 64
        static let spellings = 16
        static let spellingLength = 64
        static let words = 8
        static let replacements = 1000
        static let heardLength = 128
        static let writeLength = 1000
        static let appLength = 64
        static let appVersionLength = 32
    }

    /// Reads at most 8 MiB + 1 byte, refusing anything larger before reading it.
    static func read(contentsOf url: URL) -> HexWordsReadResult {
        if let size = try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size > maxBytes {
            return .refused(.tooLarge)
        }
        guard let handle = try? FileHandle(forReadingFrom: url) else { return .refused(.empty) }
        defer { try? handle.close() }
        let data = (try? handle.read(upToCount: maxBytes + 1)) ?? Data()
        return read(data)
    }

    static func read(_ data: Data) -> HexWordsReadResult {
        guard data.count <= maxBytes else { return .refused(.tooLarge) }
        var bytes = [UInt8](data)
        if bytes.starts(with: [0xEF, 0xBB, 0xBF]) { bytes.removeFirst(3) }
        // Swift's decoder repairs any bad byte, overlong form or encoded surrogate, so the bytes
        // come back unchanged only when they were valid UTF-8.
        guard Array(String(decoding: bytes, as: UTF8.self).utf8) == bytes else { return .refused(.notUTF8) }
        guard bytes.contains(where: { $0 != 0x20 && $0 != 0x09 && $0 != 0x0A && $0 != 0x0D }) else {
            return .refused(.empty)
        }
        if let refusal = HexWordsCheck.check(bytes) { return .refused(refusal) }
        guard let root = try? JSONSerialization.jsonObject(with: Data(bytes), options: [.fragmentsAllowed]) else {
            return .refused(.notJSON)
        }
        // NSDictionary, never [String: Any]: two keys that differ only in normalization are two
        // keys here, and bridging them into one Swift dictionary would trap.
        guard let object = root as? NSDictionary else { return .refused(.notAnObject) }
        guard let format = object.value(for: "format") as? String, format == Self.format,
              format.utf8.elementsEqual(Self.format.utf8) else {
            return .refused(.notHexWords)
        }
        guard let version = wholeNumber(object.value(for: "version")), version >= 1 else {
            return .refused(.badVersion)
        }

        var file = HexWordsFile(version: version)
        var skipped: [HexWordsSkip] = []
        let known: Set<String> = ["format", "version", "madeBy", "vocabulary", "replacements"]
        for key in object.stringKeys where !known.contains(key) {
            skipped.append(HexWordsSkip(path: key, reason: .unknownField))
        }
        if let madeBy = object.value(for: "madeBy") {
            readMadeBy(madeBy, into: &file, skipped: &skipped)
        }
        if let vocabulary = object.value(for: "vocabulary") {
            if let entries = vocabulary as? NSArray {
                file.vocabulary = readVocabulary(entries, skipped: &skipped)
            } else {
                skipped.append(HexWordsSkip(path: "vocabulary", reason: .badPart))
            }
        }
        if let replacements = object.value(for: "replacements") {
            if let entries = replacements as? NSArray {
                file.replacements = readReplacements(entries, skipped: &skipped)
            } else {
                skipped.append(HexWordsSkip(path: "replacements", reason: .badPart))
            }
        }
        file.skipped = skipped
        return .read(file)
    }

    // MARK: Entry rules, shared with the writer

    /// The first rule a term breaks, after cleaning.
    static func termProblem(_ term: String) -> HexWordsSkipReason? {
        if HexWordsText.hasHidden(term) { return .hiddenCharacters }
        if !HexWordsText.hasLetterOrDigit(term) { return .noLetters }
        if HexWordsText.length(term) > Limit.termLength { return .tooLong }
        return nil
    }

    static func spellingProblem(_ spelling: String) -> HexWordsSkipReason? {
        if HexWordsText.hasHidden(spelling) { return .hiddenCharacters }
        if !HexWordsText.hasLetterOrDigit(spelling) { return .noLetters }
        if HexWordsText.length(spelling) > Limit.spellingLength { return .tooLong }
        if HexWordsText.wordCount(spelling) > Limit.words { return .tooManyWords }
        return nil
    }

    /// The first rule a replacement breaks, after cleaning `heard` as a line and `write` as an output.
    static func replacementProblem(heard: String, write: String) -> HexWordsSkipReason? {
        if HexWordsText.hasHidden(heard) || HexWordsText.hasHidden(write, inWrite: true) { return .hiddenCharacters }
        if !HexWordsText.hasLetterOrDigit(heard) { return .noLetters }
        if HexWordsText.length(heard) > Limit.heardLength || HexWordsText.length(write) > Limit.writeLength {
            return .tooLong
        }
        if HexWordsText.wordCount(heard) > Limit.words { return .tooManyWords }
        return nil
    }

    // MARK: Parts

    private static func readMadeBy(_ value: Any, into file: inout HexWordsFile, skipped: inout [HexWordsSkip]) {
        guard let madeBy = value as? NSDictionary else {
            skipped.append(HexWordsSkip(path: "madeBy", reason: .badField))
            return
        }
        for key in madeBy.stringKeys {
            let limit: Int
            switch key {
            case "app": limit = Limit.appLength
            case "version": limit = Limit.appVersionLength
            default:
                skipped.append(HexWordsSkip(path: "madeBy." + key, reason: .unknownField))
                continue
            }
            guard let raw = madeBy.value(for: key) as? String else {
                skipped.append(HexWordsSkip(path: "madeBy." + key, reason: .badField))
                continue
            }
            let text = HexWordsText.cleanLine(raw)
            guard !text.isEmpty, !HexWordsText.hasHidden(text), HexWordsText.length(text) <= limit else {
                skipped.append(HexWordsSkip(path: "madeBy." + key, reason: .badField))
                continue
            }
            if key == "app" { file.madeByApp = text } else { file.madeByVersion = text }
        }
    }

    private static func readVocabulary(_ entries: NSArray, skipped: inout [HexWordsSkip]) -> [HexWordsTerm] {
        var terms: [HexWordsTerm] = []
        var keys: Set<String> = []
        for (index, value) in entries.enumerated() {
            let path = "vocabulary[\(index)]"
            func skip(_ reason: HexWordsSkipReason) { skipped.append(HexWordsSkip(path: path, reason: reason)) }
            guard terms.count < Limit.terms else { skip(.overLimit); continue }
            guard let entry = value as? NSDictionary, let raw = entry.value(for: "term") as? String else {
                skip(.badEntry)
                continue
            }
            let term = HexWordsText.cleanLine(raw)
            if let problem = termProblem(term) { skip(problem); continue }
            let key = HexWordsText.key(term)
            guard keys.insert(key).inserted else { skip(.duplicate); continue }

            var accepted = HexWordsTerm(term: term)
            for field in entry.stringKeys where field != "term" && field != "soundsLike" {
                skipped.append(HexWordsSkip(path: path + "." + field, reason: .unknownField))
            }
            if let soundsLike = entry.value(for: "soundsLike") {
                if let spellings = soundsLike as? NSArray {
                    accepted.soundsLike = readSpellings(spellings, termKey: key, path: path, skipped: &skipped)
                } else {
                    skipped.append(HexWordsSkip(path: path + ".soundsLike", reason: .badField))
                }
            }
            terms.append(accepted)
        }
        return terms
    }

    private static func readSpellings(_ values: NSArray, termKey: String, path: String, skipped: inout [HexWordsSkip]) -> [String] {
        var spellings: [String] = []
        var keys: Set<String> = []
        for (index, value) in values.enumerated() {
            let place = path + ".soundsLike[\(index)]"
            func skip(_ reason: HexWordsSkipReason) { skipped.append(HexWordsSkip(path: place, reason: reason)) }
            guard spellings.count < Limit.spellings else { skip(.overLimit); continue }
            guard let raw = value as? String else { skip(.badEntry); continue }
            let spelling = HexWordsText.cleanLine(raw)
            if let problem = spellingProblem(spelling) { skip(problem); continue }
            let key = HexWordsText.key(spelling)
            guard key != termKey else { skip(.sameAsTerm); continue }
            guard keys.insert(key).inserted else { skip(.duplicate); continue }
            spellings.append(spelling)
        }
        return spellings
    }

    private static func readReplacements(_ entries: NSArray, skipped: inout [HexWordsSkip]) -> [HexWordsReplacement] {
        var replacements: [HexWordsReplacement] = []
        var keys: Set<String> = []
        for (index, value) in entries.enumerated() {
            let path = "replacements[\(index)]"
            func skip(_ reason: HexWordsSkipReason) { skipped.append(HexWordsSkip(path: path, reason: reason)) }
            guard replacements.count < Limit.replacements else { skip(.overLimit); continue }
            guard let entry = value as? NSDictionary,
                  let rawHeard = entry.value(for: "heard") as? String,
                  let rawWrite = entry.value(for: "write") as? String else {
                skip(.badEntry)
                continue
            }
            let heard = HexWordsText.cleanLine(rawHeard)
            let write = HexWordsText.cleanWrite(rawWrite)
            if let problem = replacementProblem(heard: heard, write: write) { skip(problem); continue }
            guard keys.insert(HexWordsText.key(heard)).inserted else { skip(.duplicate); continue }

            var accepted = HexWordsReplacement(heard: heard, write: write)
            for field in entry.stringKeys where !["heard", "write", "learned"].contains(field) {
                skipped.append(HexWordsSkip(path: path + "." + field, reason: .unknownField))
            }
            if let learned = entry.value(for: "learned") {
                if let flag = boolean(learned) {
                    accepted.learned = flag
                } else {
                    skipped.append(HexWordsSkip(path: path + ".learned", reason: .badField))
                }
            }
            replacements.append(accepted)
        }
        return replacements
    }

    // MARK: Values

    /// `JSONSerialization` makes booleans and numbers both `NSNumber`; only a CFBoolean is a boolean.
    private static func boolean(_ value: Any) -> Bool? {
        guard let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() else { return nil }
        return number.boolValue
    }

    /// A number (never a boolean) that is whole: `1.0` is 1; `"1"`, `true` and `1.5` aren't.
    private static func wholeNumber(_ value: Any?) -> Int? {
        guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
        let double = number.doubleValue
        guard double.isFinite, double == double.rounded() else { return nil }
        return Int(exactly: double) ?? (double > 0 ? Int.max : Int.min)
    }
}

private extension NSDictionary {
    func value(for key: String) -> Any? {
        object(forKey: key as NSString)
    }

    /// Every key, as written (JSON keys are always strings).
    var stringKeys: [String] {
        allKeys.compactMap { ($0 as? NSString).map { String($0) } }
    }
}
