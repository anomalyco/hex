import CoreGraphics
import Foundation
@testable import HEX

/// Runs one parity case: a list of operations on a fresh `ACOracle` (settings, your words,
/// the keys and taps, then the questions the keyboard asks), each answered with its result and
/// the engine's state after it. The Rust test (`autocorrect_parity.rs`) replays the same list.
@MainActor
struct ACParityRunner {
    let dictionaries: [String: ACStubDictionary]
    let models: [String: TapOracleModel]
    let keysets: [Int: TapOracle.Keys]

    struct Result {
        var outputs: [Any] = []
        var hash: UInt64 = 0
        var calls = 0
        var lines: [String]?
    }

    static func language(_ code: Any?) -> KeyboardLanguage? {
        (code as? String).flatMap(KeyboardLanguage.init(rawValue:))
    }

    static func string(_ v: Any?) -> String? { v as? String }

    static func points(_ v: Any?) -> [CGPoint]? {
        guard let list = v as? [[String]] else { return nil }
        return list.map { CGPoint(x: Double(bitPattern: UInt64($0[0], radix: 16)!), y: Double(bitPattern: UInt64($0[1], radix: 16)!)) }
    }

    static func point(_ v: Any?) -> CGPoint {
        let p = v as! [String]
        return CGPoint(x: Double(bitPattern: UInt64(p[0], radix: 16)!), y: Double(bitPattern: UInt64(p[1], radix: 16)!))
    }

    /// A case's dictionaries: the exporter's standard list (`"all"`) or its own.
    static func available(_ v: Any?) -> [String] {
        (v as? String) == "all" ? ACParityGenerator.available : v as! [String]
    }

    static func field(_ v: Any?) -> KeyboardFieldKind {
        switch v as? String {
        case "url": .url
        case "email": .email
        default: .text
        }
    }

    func run(_ ops: [[String: Any]], initialLearned: [String], keepLines: Bool = false) -> Result {
        let log = ACCallLog()
        if keepLines { log.lines = [] }
        let storage = ACOLearnedWords.Storage()
        if !initialLearned.isEmpty {
            storage.data[ACOLearnedWords.defaultsKey] = try! JSONEncoder().encode(initialLearned)
        }
        let engine = ACOracle(
            checker: ACStubChecker(tag: "p", log: log, dictionaries: dictionaries, acceptsEverything: false),
            learnedWords: ACOLearnedWords(defaults: storage)
        )
        var result = Result()
        var lastLearned: [String]?
        var lastPartnerLearned: [String]?
        var lastPartner: ObjectIdentifier?
        var lastState: String?
        for op in ops {
            var out: [String: Any] = ["r": perform(op, on: engine, storage: storage, log: log)]
            // The engine's state after the operation, written when it changed.
            let state = [
                engine.lastFixLanguage?.rawValue ?? NSNull(), engine.lastRevisionLanguage?.rawValue ?? NSNull(),
                engine.ownWordsRevision, engine.partner?.language.rawValue ?? NSNull(), engine.hasDictionary,
            ] as [Any]
            let key = String(data: try! JSONSerialization.data(withJSONObject: state), encoding: .utf8)!
            if key != lastState {
                out["s"] = state
                lastState = key
            }
            let words = engine.learnedWords.words
            if words != lastLearned {
                out["lw"] = words
                lastLearned = words
            }
            let partnerID = engine.partner.map(ObjectIdentifier.init)
            if let partner = engine.partner {
                let theirs = partner.learnedWords.words
                if theirs != lastPartnerLearned || partnerID != lastPartner {
                    out["plw"] = theirs
                    lastPartnerLearned = theirs
                }
            } else {
                lastPartnerLearned = nil
            }
            lastPartner = partnerID
            result.outputs.append(out)
        }
        result.hash = log.hash
        result.calls = log.count
        result.lines = log.lines
        return result
    }

    private func perform(_ op: [String: Any], on e: ACOracle, storage: ACOLearnedWords.Storage, log: ACCallLog) -> Any {
        let s = Self.string
        switch op["op"] as! String {
        case "lang":
            e.setLanguage(Self.language(op["l"])!, available: Self.available(op["avail"]))
        case "keys":
            e.setKeys(Self.language(op["l"])!, letters: KeyboardLetterLayout(rawValue: op["letters"] as! String)!)
        case "partner":
            let checker = ACStubChecker(tag: op["tag"] as! String, log: log, dictionaries: dictionaries, acceptsEverything: op["any"] as? Bool ?? false)
            e.setPartner(Self.language(op["l"]), available: Self.available(op["avail"]), checker: checker)
        case "perlang": e.learnsPerLanguage = op["on"] as! Bool
        case "enabled": e.isEnabled = op["on"] as! Bool
        case "swear": e.allowsSwearing = op["on"] as! Bool
        case "caps": e.addsCapitals = op["on"] as! Bool
        case "lexicon":
            e.loadLexiconEntries((op["entries"] as! [[String]]).map { (userInput: $0[0], documentText: $0[1]) })
        case "vocab": e.setVocabulary(op["terms"] as! [String])
        case "learning":
            func learning(_ typos: Any?, _ rejected: Any?) -> ACOLearning {
                ACOLearning(
                    typos: Dictionary((typos as? [[String]] ?? []).map { ($0[0], $0[1]) }, uniquingKeysWith: { _, b in b }),
                    rejected: Set(rejected as? [String] ?? [])
                )
            }
            let partner = op["ptypos"] == nil ? nil : learning(op["ptypos"], op["prejected"])
            e.applyLearning(learning(op["typos"], op["rejected"]), partner: partner)
        case "decoder": e.decoder = s(op["model"]).flatMap { models[$0] }.map { TapOracle(model: $0) }
        case "pdecoder": e.partner?.decoder = s(op["model"]).flatMap { models[$0] }.map { TapOracle(model: $0) }
        case "context": e.contextModel = s(op["model"]).flatMap { models[$0] }
        case "dkeys": e.decoderKeys = (op["keys"] as? Int).flatMap { keysets[$0] }
        case "bkeys": e.decoderBaselineKeys = (op["keys"] as? Int).flatMap { keysets[$0] }
        case "taps": e.decoderTaps = Self.points(op["taps"])
        case "german": e.germanCapitals = (op["set"] as? String) == "bundled" ? ACOGermanCapitals.bundled() : nil
        case "keep": e.suppressAndKeep(op["w"] as! String)
        case "erase":
            storage.now += 1
            storage.erasedAt = storage.now
        case "suggest":
            let model = s(op["model"]).flatMap { models[$0] }
            return e.suggestions(
                beforeInput: s(op["before"]), field: Self.field(op["field"]),
                autocorrectionType: op["off"] as? Bool == true ? .no : .default,
                predictionModel: model, capitalizesNext: op["cap"] as? Bool ?? false
            ).map { suggestion -> [Any] in
                let kind: String = switch suggestion.kind {
                case .keepLiteral: "literal"
                case .primary(let auto): auto ? "auto" : "primary"
                case .secondary: "secondary"
                case .nextWord: "next"
                default: "other"
                }
                return [suggestion.displayText, suggestion.replacement, kind]
            }
        case "space":
            let fix = e.replacementOnSpace(
                beforeInput: s(op["before"]), field: Self.field(op["field"]),
                autocorrectionType: op["off"] as? Bool == true ? .no : .default
            )
            return fix.map { [$0.original, $0.replacement] } ?? NSNull()
        case "revision":
            return e.revision(
                of: s(op["w"])!, before: s(op["before"])!, next: s(op["next"])!, field: Self.field(op["field"]),
                autocorrectionType: op["off"] as? Bool == true ? .no : .default
            ) ?? NSNull()
        case "merge":
            return e.merge(
                first: s(op["first"])!, firstTaps: Self.points(op["ftaps"]) ?? [], space: Self.point(op["space"]),
                second: s(op["second"])!, before: s(op["before"])!
            ) ?? NSNull()
        case "replacement":
            return e.textReplacement(before: s(op["before"])).map { [$0.typed, $0.replacement] } ?? NSNull()
        case "offer": return e.mayOffer(s(op["w"])!, after: s(op["after"]))
        case "knows": return e.knowsAsTyped(s(op["w"])!)
        case "lean":
            guard let partner = e.partner else { return NSNull() }
            return e.languageLean(partner, before: s(op["before"])!, commonFrom: op["from"] as! Int)
        case "choice":
            return e.contextChoice(for: s(op["w"])!, previous: s(op["prev"]), next: s(op["next"]), startsSentence: op["starts"] as! Bool) ?? NSNull()
        case "stray": return e.withoutStrayCapital(s(op["w"])!, typed: s(op["typed"])!)
        case "added": return e.withoutAddedCapitals(s(op["w"])!, typed: s(op["typed"])!) ?? NSNull()
        case "addedpool": return e.withoutAddedCapitals(op["pool"] as! [String], typed: s(op["typed"])!)
        case "ownspelling": return e.isOwnSpelling(s(op["w"])!)
        case "ownwords": return e.ownWords
        case "vterm": return e.vocabularyTerm(for: s(op["w"])!) ?? NSNull()
        case "record":
            let changes = (op["changes"] as! [[Any]]).map {
                ACOracle.AppliedCorrection.Change(from: $0[0] as! String, to: $0[1] as! String, language: Self.language($0[2]))
            }
            e.recordAppliedCorrection(original: s(op["original"])!, corrected: s(op["corrected"])!, anchor: s(op["anchor"])!, changes: changes)
        case "revertible":
            guard let c = e.revertibleCorrection(before: s(op["before"])) else { return NSNull() }
            return [c.original, c.corrected, c.anchor, c.changes.map { [$0.from, $0.to, $0.language?.rawValue as Any? ?? NSNull()] as [Any] }] as [Any]
        case "forget": e.forgetCorrection()
        case "blocked": return e.offensiveWords.blocked(allowSwearing: e.allowsSwearing).sorted()
        case "token": return ACOracle.currentToken(in: s(op["before"])) ?? NSNull()
        case "sentence": return ACOracle.isSentenceStart(s(op["before"]), language: Self.language(op["l"])!)
        case "prevword": return ACOracle.previousWord(s(op["before"])!, token: s(op["token"])!) ?? NSNull()
        case "starts": return ACOracle.startsSentence(before: s(op["before"])!, token: s(op["token"])!)
        case "matchcase": return ACOracle.matchCase(of: s(op["typed"])!, onto: s(op["target"])!)
        case "cost":
            let cost = ACOracle.singleEditKeyboardCost(
                from: s(op["s"])!, to: s(op["t"])!, language: Self.language(op["l"])!,
                letters: s(op["letters"]).flatMap(KeyboardLetterLayout.init(rawValue:))
            )
            return cost.map { String($0.bitPattern, radix: 16) } ?? NSNull()
        case "vowel": return ACOracle.hasVowel(s(op["w"])!)
        case "vspell": return ACOracle.vocabularySpelling(of: s(op["token"])!, as: s(op["term"])!) ?? NSNull()
        default:
            fatalError("unknown op \(op["op"]!)")
        }
        return NSNull()
    }
}
