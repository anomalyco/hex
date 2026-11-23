import Foundation

/// A typing config file (`.mhexconfig`), format 0, as written: the envelope, words and key
/// positions with the agent's evidence and note (`ios/TypingConfig/package/reference/
/// typing-config.md`). Decoding is lenient: an entry that doesn't decode is kept as `nil` in
/// its list, so `TypingConfigReview` can name it and read the rest; unknown keys are found
/// there too. `TypingConfigReview.compiled` writes the same model back as `active.json`.
nonisolated struct TypingConfigFile: Codable, Equatable, Sendable {
    var format: String
    var version: Int
    var id: String?
    var basedOn: String?
    var madeFrom: MadeFrom?
    var madeFor: MadeFor?
    var note: String?
    var words: Words?
    var keys: [Lossy<KeyBlock>]?

    struct MadeFrom: Codable, Equatable, Sendable {
        var exportId: String?
        var hexVersion: String?
        var hexBuild: String?
        var logDays: [String]?
        var heldOut: HeldOut?
    }

    struct HeldOut: Codable, Equatable, Sendable {
        var tunedOn: String?
        var checkedOn: String?
    }

    struct MadeFor: Codable, Equatable, Sendable {
        var device: String?
        var language: String?
    }

    struct Words: Codable, Equatable, Sendable {
        var keep: [Lossy<WordEntry>]?
        var neverCorrectTo: [Lossy<WordEntry>]?
        var fixes: [Lossy<FixEntry>]?
        var dictation: [Lossy<DictationEntry>]?
    }

    struct WordEntry: Codable, Equatable, Sendable {
        var word: String
        var evidence: Evidence?
    }

    enum Mode: String, Codable, Equatable, Sendable {
        /// Space puts the word in (only offered when the typo is itself a word).
        case apply
        /// Offered in the strip, never put in by space.
        case suggest
    }

    struct FixEntry: Codable, Equatable, Sendable {
        var typo: String
        var word: String
        var mode: Mode
        var evidence: Evidence?
    }

    struct DictationEntry: Codable, Equatable, Sendable {
        var heard: String
        var meant: String
        var evidence: Evidence?
    }

    /// What the agent says it saw. Never trusted: Hex checks a config on the person's own logs.
    struct Evidence: Codable, Equatable, Sendable {
        var times: Int?
        var sessions: Int?
        var takenBack: Int?
        var tuned: Tally?
        var checked: Tally?
    }

    struct Tally: Codable, Equatable, Sendable {
        var taps: Int?
        var gained: Int?
        var lost: Int?
        /// For a zone: the words its taps' changes are expected to make right and wrong
        /// (`simulate_zones.py`).
        var wordsRight: Double?
        var wordsWrong: Double?
    }

    /// Key positions for one keyboard (`Context`): aim by letter, and zones.
    struct KeyBlock: Codable, Equatable, Sendable {
        var context: Context
        var aim: [String: Lossy<Aim>]?
        var zones: [Lossy<Zone>]?
    }

    /// The keyboard a key block was measured on (`TypingConfigContext` reads the live one).
    struct Context: Codable, Equatable, Sendable {
        var width: Double
        var orientation: String
        var posture: String
        var language: String
        var letters: String
        var split: Bool
        var hexPlacement: String
        var numberRow: Bool
        var keyHeight: Double
        var rowPitch: Double
        var letterWidth: Double
        var hitModel: Int
    }

    /// Where the person aims for a letter: `[dx, dy]` in its keycap's width and height.
    struct Aim: Codable, Equatable, Sendable {
        var offset: [Double]
        var evidence: Evidence?
    }

    /// One moved edge (`KeyboardZone`): `move` letter keycaps, positive grows the key.
    struct Zone: Codable, Equatable, Sendable {
        var key: String
        var edge: String
        var move: Double
        var evidence: Evidence?
    }

    /// An element that may fail to decode without failing the file: `value` is nil then.
    struct Lossy<Value: Codable & Equatable & Sendable>: Codable, Equatable, Sendable {
        var value: Value?

        init(_ value: Value?) { self.value = value }

        init(from decoder: Decoder) throws {
            value = try? Value(from: decoder)
        }

        func encode(to encoder: Encoder) throws {
            var container = encoder.singleValueContainer()
            try container.encode(value)
        }
    }

    /// The file's keys at each level, for finding the ones Hex doesn't know.
    static let knownKeys: [String: Set<String>] = [
        "": ["format", "version", "id", "basedOn", "madeFrom", "madeFor", "note", "words", "keys"],
        "madeFrom": ["exportId", "hexVersion", "hexBuild", "logDays", "heldOut"],
        "madeFrom.heldOut": ["tunedOn", "checkedOn"],
        "madeFor": ["device", "language"],
        "words": ["keep", "neverCorrectTo", "fixes", "dictation"],
        "words.keep[]": ["word", "evidence"],
        "words.neverCorrectTo[]": ["word", "evidence"],
        "words.fixes[]": ["typo", "word", "mode", "evidence"],
        "words.dictation[]": ["heard", "meant", "evidence"],
        "keys[]": ["context", "aim", "zones"],
        "keys[].context": ["width", "orientation", "posture", "language", "letters", "split", "hexPlacement", "numberRow",
                           "keyHeight", "rowPitch", "letterWidth", "hitModel"],
        "keys[].aim.*": ["offset", "evidence"],
        "keys[].zones[]": ["key", "edge", "move", "evidence"],
        "evidence": ["times", "sessions", "takenBack", "tuned", "checked"],
        "tally": ["taps", "gained", "lost", "wordsRight", "wordsWrong"],
    ]
}
