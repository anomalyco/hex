import CoreGraphics
import Foundation
import HexKeyboardCore
import XCTest
@testable import HEX

/// The shipped autocorrect (`AutocorrectEngine` over the keyboard core) against the parity
/// fixture (`crates/keyboard/core/tests/fixtures/autocorrect-parity.jsonl`): every case's
/// operations, from the fixture alone, answered and asked of the dictionary as recorded. The
/// fixture was recorded from dev's Swift engine with iOS's own never-suggest list (`out`) and
/// with the shared one iOS ships now (`shared`, where they differ); the engine reads the shared one.
///
/// When a change is meant to change autocorrect's answers, write the fixture again from the
/// shipped engine with `TEST_RUNNER_HEX_AUTOCORRECT_PARITY_REWRITE=<path>`: the same cases, the
/// engine's answers as `out`, and the never-suggest list iOS ships as `iosNeverSuggest`.
@MainActor
final class AutocorrectParityGolden: XCTestCase {
    func testTheShippedAutocorrectAnswersAsRecorded() throws {
        let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().path
        let text = try String(contentsOfFile: repo + "/crates/keyboard/core/tests/fixtures/autocorrect-parity.jsonl", encoding: .utf8)
        let rewrite = ProcessInfo.processInfo.environment["HEX_AUTOCORRECT_PARITY_REWRITE"]
        var rewritten: [String] = []
        func keep(_ object: [String: Any]) throws {
            let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys, .withoutEscapingSlashes])
            rewritten.append(String(decoding: data, as: UTF8.self))
        }
        var runner: ACWrapperRunner?
        var keysets: [Int: TypingDecoder.Keys] = [:]
        var cases = 0, differ = 0
        for line in text.split(separator: "\n") {
            var v = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
            switch v["kind"] as? String {
            case "setup":
                if rewrite != nil {
                    let shipped = Bundle.main.url(forResource: "never-suggest", withExtension: "tsv")
                    v["iosNeverSuggest"] = try shipped.map { try String(contentsOf: $0, encoding: .utf8) } ?? ""
                    try keep(v)
                }
                var models: [String: TypingDecoderModel] = [:]
                for code in v["models"] as! [String] {
                    let url = URL(fileURLWithPath: repo + "/ios/HEXKeyboard/TypingModel/\(code).hextm")
                    models[code] = try XCTUnwrap(TypingDecoderModel(url: url), "no \(code) model")
                }
                runner = ACWrapperRunner(dictionaries: ACStub.dictionaries(setup: v, models: models), models: models, keysets: [:])
            case "keys":
                keysets[v["id"] as! Int] = ACWrapperRunner.keys(v["keys"] as! [String: Any])
                if rewrite != nil { rewritten.append(String(line)) }
            case "case":
                guard var runner else { return XCTFail("a setup line first") }
                runner.keysets = keysets
                cases += 1
                let result = runner.run(v["ops"] as! [[String: Any]], initialLearned: v["learned"] as! [String])
                let expected = (v["shared"] ?? v["out"]) as! [Any]
                let calls = (v["sharedCalls"] ?? v["calls"]) as! String
                if rewrite != nil {
                    for key in ["today", "tie", "foundation", "shared", "sharedCalls"] { v[key] = nil }
                    v["out"] = result.outputs
                    v["calls"] = String(result.hash, radix: 16)
                    v["ncalls"] = result.calls
                    try keep(v)
                    continue
                }
                if !ACWrapperRunner.same(result.outputs, expected) || String(result.hash, radix: 16) != calls {
                    differ += 1
                    if differ <= 3 {
                        XCTFail("case \(v["id"]!) (\(v["lang"]!)): \(ACWrapperRunner.firstDifference(result.outputs, expected, ops: v["ops"] as! [[String: Any]]))")
                    }
                }
            default:
                if rewrite != nil { rewritten.append(String(line)) }
            }
        }
        if let rewrite {
            try (rewritten.joined(separator: "\n") + "\n").write(toFile: rewrite, atomically: true, encoding: .utf8)
            return
        }
        XCTAssertGreaterThan(cases, 300)
        XCTAssertEqual(differ, 0, "of \(cases) cases")
    }
}

/// Runs one parity case on a fresh `AutocorrectEngine`, as `autocorrect_parity.rs` does on the
/// core: the operations in order, each answered with its result and the engine's state after it.
@MainActor
struct ACWrapperRunner {
    let dictionaries: [String: ACStubDictionary]
    let models: [String: TypingDecoderModel]
    var keysets: [Int: TypingDecoder.Keys]
    /// Apple's dictionary (`SystemTextChecker`) for every engine instead of the stub.
    var system = false

    struct Result {
        var outputs: [Any] = []
        var hash: UInt64 = 0
        var calls = 0
    }

    /// One defaults suite every case starts empty.
    static let suite = "anomaly.hex.tests.autocorrect-parity"

    static func keys(_ v: [String: Any]) -> TypingDecoder.Keys {
        func bits(_ s: Any?) -> Double { Double(bitPattern: UInt64(s as! String, radix: 16)!) }
        var centres: [Character: CGPoint] = [:]
        for c in v["centres"] as! [[String]] {
            centres[Character(c[0])] = CGPoint(x: bits(c[1]), y: bits(c[2]))
        }
        let pitch = v["pitch"] as! [String]
        let space = (v["space"] as? [String]).map { CGRect(x: bits($0[0]), y: bits($0[1]), width: bits($0[2]), height: bits($0[3])) }
        return TypingDecoder.Keys(centres: centres, pitch: CGSize(width: bits(pitch[0]), height: bits(pitch[1])), bottomRowY: bits(v["bottom"]), space: space)
    }

    static func same(_ a: [Any], _ b: [Any]) -> Bool {
        let options: JSONSerialization.WritingOptions = [.sortedKeys]
        return (try? JSONSerialization.data(withJSONObject: a, options: options)) == (try? JSONSerialization.data(withJSONObject: b, options: options))
    }

    static func firstDifference(_ a: [Any], _ b: [Any], ops: [[String: Any]]) -> String {
        for i in 0..<max(a.count, b.count) where !same([i < a.count ? a[i] : NSNull()], [i < b.count ? b[i] : NSNull()]) {
            return "op \(i) \(ops[i]): engine \(i < a.count ? a[i] : "-") recorded \(i < b.count ? b[i] : "-")"
        }
        return "the dictionary was asked otherwise"
    }

    static func grid(_ language: KeyboardLanguage, _ letters: KeyboardLetterLayout?) -> String {
        language.rawValue + "/" + (letters?.rawValue ?? "-")
    }

    static func language(_ code: Any?) -> KeyboardLanguage? {
        (code as? String).flatMap(KeyboardLanguage.init(rawValue:))
    }

    static func points(_ v: Any?) -> [CGPoint]? {
        guard let list = v as? [[String]] else { return nil }
        return list.map { CGPoint(x: Double(bitPattern: UInt64($0[0], radix: 16)!), y: Double(bitPattern: UInt64($0[1], radix: 16)!)) }
    }

    static func point(_ v: Any?) -> CGPoint {
        points([v as! [String]])![0]
    }

    static let available = ["en_US", "en", "es", "de", "it", "fr", "fr_CA", "fr_CH", "pt_BR", "pl", "cs", "hu", "ro", "ru", "uk", "el", "nb"]

    static func available(_ v: Any?) -> [String] {
        (v as? String) == "all" ? available : v as! [String]
    }

    static func field(_ v: Any?) -> KeyboardFieldKind {
        switch v as? String {
        case "url": .url
        case "email": .email
        default: .text
        }
    }

    func run(_ ops: [[String: Any]], initialLearned: [String]) -> Result {
        let log = ACCallLog()
        let defaults = UserDefaults(suiteName: Self.suite)!
        defaults.removePersistentDomain(forName: Self.suite)
        if !initialLearned.isEmpty {
            defaults.set(try! JSONEncoder().encode(initialLearned), forKey: LearnedWordsStore.defaultsKey)
        }
        let engine = AutocorrectEngine(
            checker: system ? SystemTextChecker() : ACStubChecker(tag: "p", log: log, dictionaries: dictionaries, acceptsEverything: false),
            learnedWords: LearnedWordsStore(defaults: defaults)
        )
        var result = Result()
        var lastGrid = Self.grid(engine.layoutLanguage, engine.layoutLetters)
        var lastLearned: [String]?
        var lastPartnerLearned: [String]?
        var lastPartner: ObjectIdentifier?
        var lastState: String?
        for op in ops {
            var out: [String: Any] = ["r": perform(op, on: engine, defaults: defaults, log: log)]
            let state = [
                engine.lastFixLanguage?.rawValue ?? NSNull(), engine.lastRevisionLanguage?.rawValue ?? NSNull(),
                engine.ownWordsRevision, engine.partner?.language.rawValue ?? NSNull(), engine.hasDictionary,
            ] as [Any]
            let key = String(data: try! JSONSerialization.data(withJSONObject: state), encoding: .utf8)!
            if key != lastState {
                out["s"] = state
                lastState = key
            }
            let grid = Self.grid(engine.layoutLanguage, engine.layoutLetters)
            if grid != lastGrid {
                out["g"] = grid
                lastGrid = grid
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
        return result
    }

    private func perform(_ op: [String: Any], on e: AutocorrectEngine, defaults: UserDefaults, log: ACCallLog) -> Any {
        func s(_ v: Any?) -> String? { v as? String }
        let off: (Any?) -> UITextAutocorrectionType = { ($0 as? Bool) == true ? .no : .default }
        switch op["op"] as! String {
        case "lang": e.setLanguage(Self.language(op["l"])!, available: Self.available(op["avail"]))
        case "keys": e.setKeys(Self.language(op["l"])!, letters: KeyboardLetterLayout(rawValue: op["letters"] as! String)!)
        case "partner":
            let checker: TextChecking = system
                ? SystemTextChecker()
                : ACStubChecker(tag: op["tag"] as! String, log: log, dictionaries: dictionaries, acceptsEverything: op["any"] as? Bool ?? false)
            e.setPartner(Self.language(op["l"]), available: Self.available(op["avail"]), checker: checker)
        case "perlang": e.learnsPerLanguage = op["on"] as! Bool
        case "enabled": e.isEnabled = op["on"] as! Bool
        case "swear": e.allowsSwearing = op["on"] as! Bool
        case "caps": e.addsCapitals = op["on"] as! Bool
        case "lexicon": e.loadLexiconEntries((op["entries"] as! [[String]]).map { (userInput: $0[0], documentText: $0[1]) })
        case "vocab": e.setVocabulary(op["terms"] as! [String])
        case "learning":
            func learning(_ typos: Any?, _ rejected: Any?) -> TypingLearning {
                TypingLearning(
                    typos: Dictionary((typos as? [[String]] ?? []).map { ($0[0], $0[1]) }, uniquingKeysWith: { _, b in b }),
                    rejected: Set(rejected as? [String] ?? [])
                )
            }
            let partner = op["ptypos"] == nil ? nil : learning(op["ptypos"], op["prejected"])
            e.applyLearning(learning(op["typos"], op["rejected"]), partner: partner)
        case "decoder": e.decoder = s(op["model"]).flatMap { models[$0] }.map { TypingDecoder(model: $0) }
        case "pdecoder": e.partner?.decoder = s(op["model"]).flatMap { models[$0] }.map { TypingDecoder(model: $0) }
        case "context": e.contextModel = s(op["model"]).flatMap { models[$0] }
        case "dkeys": e.decoderKeys = (op["keys"] as? Int).flatMap { keysets[$0] }
        case "bkeys": e.decoderBaselineKeys = (op["keys"] as? Int).flatMap { keysets[$0] }
        case "taps": e.decoderTaps = Self.points(op["taps"])
        case "german": e.germanCapitals = (op["set"] as? String) == "bundled" ? GermanCapitals.bundled() : nil
        case "keep": e.suppressAndKeep(op["w"] as! String)
        case "erase":
            // Delete what Hex learned, now: every store read before drops its words, one made
            // after keeps them.
            usleep(10)
            TypingJournalSettings(defaults: defaults).erasedAt = Date().timeIntervalSince1970
            usleep(10)
        case "suggest":
            return e.suggestions(
                beforeInput: s(op["before"]), field: Self.field(op["field"]), autocorrectionType: off(op["off"]),
                predictionModel: s(op["model"]).flatMap { models[$0] }, capitalizesNext: op["cap"] as? Bool ?? false
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
            let fix = e.replacementOnSpace(beforeInput: s(op["before"]), field: Self.field(op["field"]), autocorrectionType: off(op["off"]))
            return fix.map { [$0.original, $0.replacement] } ?? NSNull()
        case "revision":
            return e.revision(
                of: s(op["w"])!, before: s(op["before"])!, next: s(op["next"])!, field: Self.field(op["field"]), autocorrectionType: off(op["off"])
            ) ?? NSNull()
        case "merge":
            return e.merge(
                first: s(op["first"])!, firstTaps: Self.points(op["ftaps"]) ?? [], space: Self.point(op["space"]),
                second: s(op["second"])!, before: s(op["before"])!
            ) ?? NSNull()
        case "replacement": return e.textReplacement(before: s(op["before"])).map { [$0.typed, $0.replacement] } ?? NSNull()
        case "offer": return e.mayOffer(s(op["w"])!, after: s(op["after"]))
        case "knows": return e.knowsAsTyped(s(op["w"])!)
        case "lean":
            guard e.partner != nil else { return NSNull() }
            e.prepare()
            var lean: Int32 = 0
            guard hk_autocorrect_language_lean(e.core.pointer, s(op["before"])!, op["from"] as! Int, &lean) == 0 else { return NSNull() }
            return Int(lean)
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
                AutocorrectEngine.AppliedCorrection.Change(from: $0[0] as! String, to: $0[1] as! String, language: Self.language($0[2]))
            }
            e.recordAppliedCorrection(original: s(op["original"])!, corrected: s(op["corrected"])!, anchor: s(op["anchor"])!, changes: changes)
        case "revertible":
            guard let c = e.revertibleCorrection(before: s(op["before"])) else { return NSNull() }
            return [c.original, c.corrected, c.anchor, c.changes.map { [$0.from, $0.to, $0.language?.rawValue as Any? ?? NSNull()] as [Any] }] as [Any]
        case "forget": e.forgetCorrection()
        case "blocked": return e.offensiveWords.blocked(allowSwearing: e.allowsSwearing).sorted()
        case "swipereadings":
            guard let model = s(op["model"]).flatMap({ models[$0] }) else { return NSNull() }
            return e.readingsOfferingOwnCapitals(op["readings"] as! [String], capsLock: op["caps"] as? Bool ?? false, model: model)
        case "haswords": return e.hasWords
        case "token": return AutocorrectEngine.currentToken(in: s(op["before"])) ?? NSNull()
        case "sentence": return AutocorrectEngine.isSentenceStart(s(op["before"]), language: Self.language(op["l"])!)
        case "prevword": return AutocorrectEngine.previousWord(s(op["before"])!, token: s(op["token"])!) ?? NSNull()
        case "starts": return AutocorrectEngine.startsSentence(before: s(op["before"])!, token: s(op["token"])!)
        case "matchcase": return KeyboardCoreText.text(2, s(op["typed"])!, s(op["target"])!) ?? NSNull()
        case "cost":
            let cost = AutocorrectEngine.singleEditKeyboardCost(
                from: s(op["s"])!, to: s(op["t"])!, language: Self.language(op["l"])!,
                letters: s(op["letters"]).flatMap(KeyboardLetterLayout.init(rawValue:))
            )
            return cost.map { String($0.bitPattern, radix: 16) } ?? NSNull()
        case "vowel": return hk_text_is(2, s(op["w"])!, nil, nil) == 1
        case "vspell": return KeyboardCoreText.text(3, s(op["token"])!, s(op["term"])!) ?? NSNull()
        default: fatalError("unknown op \(op["op"]!)")
        }
        return NSNull()
    }
}
